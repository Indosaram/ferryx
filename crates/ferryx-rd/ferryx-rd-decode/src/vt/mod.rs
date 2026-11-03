pub mod ffi;

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use crate::{DecodeError, HardwareAcceleration, Nv12Frame, NAL_LENGTH_BYTES};

struct SharedState {
    frames: Mutex<Vec<Nv12Frame>>,
    error: Mutex<Option<DecodeError>>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            frames: Mutex::new(Vec::new()),
            error: Mutex::new(None),
        }
    }

    fn on_frame(
        &self,
        source_frame_refcon: *mut c_void,
        status: ffi::OSStatus,
        info_flags: ffi::VTDecodeInfoFlags,
        image_buffer: ffi::CVImageBufferRef,
        presentation_time_stamp: ffi::CMTime,
    ) {
        if status != 0 {
            let mut err_guard = self.error.lock().unwrap();
            if err_guard.is_none() {
                *err_guard = Some(DecodeError::VideoToolbox(format!(
                    "decompression callback failed: status {status}"
                )));
            }
            return;
        }

        if (info_flags & ffi::K_VTDECODE_INFO_FRAME_DROPPED) != 0 || image_buffer.is_null() {
            return;
        }

        let timestamp_ms = if presentation_time_stamp.timescale == 1000 {
            presentation_time_stamp.value
        } else if presentation_time_stamp.timescale > 0 {
            (presentation_time_stamp.value * 1000) / (presentation_time_stamp.timescale as i64)
        } else {
            source_frame_refcon as usize as i64
        };

        match copy_cv_pixel_buffer_to_nv12(image_buffer, timestamp_ms) {
            Ok(frame) => {
                self.frames.lock().unwrap().push(frame);
            }
            Err(e) => {
                let mut err_guard = self.error.lock().unwrap();
                if err_guard.is_none() {
                    *err_guard = Some(e);
                }
            }
        }
    }

    fn take_frames(&self) -> Vec<Nv12Frame> {
        std::mem::take(&mut *self.frames.lock().unwrap())
    }

    fn take_error(&self) -> Option<DecodeError> {
        self.error.lock().unwrap().take()
    }
}

fn copy_cv_pixel_buffer_to_nv12(
    pixel_buffer: ffi::CVPixelBufferRef,
    timestamp_ms: i64,
) -> Result<Nv12Frame, DecodeError> {
    if pixel_buffer.is_null() {
        return Err(DecodeError::UnsupportedFrame);
    }
    unsafe {
        let lock_status =
            ffi::CVPixelBufferLockBaseAddress(pixel_buffer, ffi::K_CVPIXEL_BUFFER_LOCK_READ_ONLY);
        if lock_status != 0 {
            return Err(DecodeError::VideoToolbox(format!(
                "CVPixelBufferLockBaseAddress failed: {lock_status}"
            )));
        }

        struct UnlockGuard(ffi::CVPixelBufferRef);
        impl Drop for UnlockGuard {
            fn drop(&mut self) {
                unsafe {
                    ffi::CVPixelBufferUnlockBaseAddress(
                        self.0,
                        ffi::K_CVPIXEL_BUFFER_LOCK_READ_ONLY,
                    );
                }
            }
        }
        let _guard = UnlockGuard(pixel_buffer);

        let format = ffi::CVPixelBufferGetPixelFormatType(pixel_buffer);
        if format != ffi::K_CVPIXEL_FORMAT_TYPE_420_YP_CB_CR8_BI_PLANAR_VIDEO_RANGE
            && format != ffi::K_CVPIXEL_FORMAT_TYPE_420_YP_CB_CR8_BI_PLANAR_FULL_RANGE
        {
            return Err(DecodeError::UnsupportedFrame);
        }

        let is_planar = ffi::CVPixelBufferIsPlanar(pixel_buffer);
        let plane_count = ffi::CVPixelBufferGetPlaneCount(pixel_buffer);
        if is_planar == 0 || plane_count < 2 {
            return Err(DecodeError::UnsupportedFrame);
        }

        let width = ffi::CVPixelBufferGetWidth(pixel_buffer);
        let height = ffi::CVPixelBufferGetHeight(pixel_buffer);
        if width == 0 || height == 0 {
            return Err(DecodeError::UnsupportedFrame);
        }

        let y_base = ffi::CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, 0) as *const u8;
        let y_stride = ffi::CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, 0);
        let uv_base = ffi::CVPixelBufferGetBaseAddressOfPlane(pixel_buffer, 1) as *const u8;
        let uv_stride = ffi::CVPixelBufferGetBytesPerRowOfPlane(pixel_buffer, 1);

        if y_base.is_null() || uv_base.is_null() || y_stride < width || uv_stride < width {
            return Err(DecodeError::UnsupportedFrame);
        }

        let uv_rows = height.div_ceil(2);
        let mut y_plane = vec![0_u8; width * height];
        let mut uv_plane = vec![0_u8; width * uv_rows];

        for row in 0..height {
            let src_row = std::slice::from_raw_parts(y_base.add(row * y_stride), width);
            y_plane[row * width..(row + 1) * width].copy_from_slice(src_row);
        }

        for row in 0..uv_rows {
            let src_row = std::slice::from_raw_parts(uv_base.add(row * uv_stride), width);
            uv_plane[row * width..(row + 1) * width].copy_from_slice(src_row);
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
}

pub struct HevcDecoder {
    session: ffi::VTDecompressionSessionRef,
    format_desc: ffi::CMVideoFormatDescriptionRef,
    shared: Arc<SharedState>,
    raw_context: *mut c_void,
}

unsafe impl Send for HevcDecoder {}

impl HevcDecoder {
    pub fn new(extradata: &[u8]) -> Result<Self, DecodeError> {
        let format_desc = create_hevc_format_description(extradata)?;
        Self::create_session(format_desc)
    }

    pub fn new_h264(extradata: &[u8]) -> Result<Self, DecodeError> {
        let format_desc = create_h264_format_description(extradata)?;
        Self::create_session(format_desc)
    }

    pub fn from_keyframe(keyframe: &[u8]) -> Result<Self, DecodeError> {
        Self::new(&crate::hevc_parameter_set_blob(keyframe)?)
    }

    pub fn from_keyframe_auto(keyframe: &[u8]) -> Result<(crate::CodecKind, Self), DecodeError> {
        let kind = crate::detect_codec(keyframe)?;
        let decoder = match kind {
            crate::CodecKind::Hevc => Self::new(&crate::hevc_parameter_set_blob(keyframe)?),
            crate::CodecKind::H264 => Self::new_h264(&crate::h264_parameter_set_blob(keyframe)?),
        }?;
        Ok((kind, decoder))
    }

    pub fn acceleration(&self) -> HardwareAcceleration {
        HardwareAcceleration::VideoToolbox
    }

    fn create_session(format_desc: ffi::CMVideoFormatDescriptionRef) -> Result<Self, DecodeError> {
        let shared = Arc::new(SharedState::new());
        let raw_context = Arc::into_raw(shared.clone()) as *mut c_void;

        let callback_record = ffi::VTDecompressionOutputCallbackRecord {
            decompression_output_callback: Some(decompression_output_callback),
            decompression_output_refcon: raw_context,
        };

        unsafe {
            // Clients render NV12 as full range, so VideoToolbox must output full range.
            let pixel_format: i32 =
                ffi::K_CVPIXEL_FORMAT_TYPE_420_YP_CB_CR8_BI_PLANAR_FULL_RANGE as i32;
            let pixel_format_num = ffi::CFNumberCreate(
                std::ptr::null(),
                ffi::K_CFNUMBER_SINT32_TYPE,
                &pixel_format as *const i32 as *const c_void,
            );

            let dest_attrs = if !pixel_format_num.is_null() {
                let keys = [ffi::kCVPixelBufferPixelFormatTypeKey as *const c_void];
                let values = [pixel_format_num as *const c_void];
                let dict = ffi::CFDictionaryCreate(
                    std::ptr::null(),
                    keys.as_ptr(),
                    values.as_ptr(),
                    1,
                    &ffi::kCFTypeDictionaryKeyCallBacks,
                    &ffi::kCFTypeDictionaryValueCallBacks,
                );
                ffi::CFRelease(pixel_format_num as ffi::CFTypeRef);
                dict
            } else {
                std::ptr::null()
            };

            let mut session: ffi::VTDecompressionSessionRef = std::ptr::null_mut();
            let status = ffi::VTDecompressionSessionCreate(
                std::ptr::null(),
                format_desc,
                std::ptr::null(),
                dest_attrs,
                &callback_record,
                &mut session,
            );

            if !dest_attrs.is_null() {
                ffi::CFRelease(dest_attrs as ffi::CFTypeRef);
            }

            if status != 0 || session.is_null() {
                ffi::CFRelease(format_desc as ffi::CFTypeRef);
                drop(Arc::from_raw(raw_context as *mut SharedState));
                return Err(DecodeError::VideoToolboxInit(format!(
                    "VTDecompressionSessionCreate failed: status {status}"
                )));
            }

            Ok(Self {
                session,
                format_desc,
                shared,
                raw_context,
            })
        }
    }

    pub fn decode(
        &mut self,
        access_unit: &[u8],
        timestamp_ms: i64,
    ) -> Result<Vec<Nv12Frame>, DecodeError> {
        crate::parse_length_prefixed_nalus(access_unit)?;

        unsafe {
            let mut block_buffer: ffi::CMBlockBufferRef = std::ptr::null_mut();
            let status = ffi::CMBlockBufferCreateWithMemoryBlock(
                std::ptr::null(),
                std::ptr::null_mut(),
                access_unit.len(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                access_unit.len(),
                0,
                &mut block_buffer,
            );
            if status != 0 || block_buffer.is_null() {
                return Err(DecodeError::VideoToolbox(format!(
                    "CMBlockBufferCreateWithMemoryBlock failed: {status}"
                )));
            }

            let replace_status = ffi::CMBlockBufferReplaceDataBytes(
                access_unit.as_ptr() as *const c_void,
                block_buffer,
                0,
                access_unit.len(),
            );
            if replace_status != 0 {
                ffi::CFRelease(block_buffer as ffi::CFTypeRef);
                return Err(DecodeError::VideoToolbox(format!(
                    "CMBlockBufferReplaceDataBytes failed: {replace_status}"
                )));
            }

            let timing = ffi::CMSampleTimingInfo {
                duration: ffi::CMTime::INVALID,
                presentation_time_stamp: ffi::CMTime::make(timestamp_ms, 1000),
                decode_time_stamp: ffi::CMTime::INVALID,
            };
            let sample_size = access_unit.len();
            let mut sample_buffer: ffi::CMSampleBufferRef = std::ptr::null_mut();

            let sample_status = ffi::CMSampleBufferCreateReady(
                std::ptr::null(),
                block_buffer,
                self.format_desc,
                1,
                1,
                &timing,
                1,
                &sample_size,
                &mut sample_buffer,
            );
            ffi::CFRelease(block_buffer as ffi::CFTypeRef);

            if sample_status != 0 || sample_buffer.is_null() {
                return Err(DecodeError::VideoToolbox(format!(
                    "CMSampleBufferCreateReady failed: {sample_status}"
                )));
            }

            let mut info_flags: ffi::VTDecodeInfoFlags = 0;
            let decode_status = ffi::VTDecompressionSessionDecodeFrame(
                self.session,
                sample_buffer,
                0,
                timestamp_ms as usize as *mut c_void,
                &mut info_flags,
            );
            ffi::CFRelease(sample_buffer as ffi::CFTypeRef);

            if decode_status != 0 {
                return Err(DecodeError::VideoToolbox(format!(
                    "VTDecompressionSessionDecodeFrame failed: {decode_status}"
                )));
            }

            if (info_flags & ffi::K_VTDECODE_INFO_ASYNCHRONOUS) != 0
                || self.shared.frames.lock().unwrap().is_empty()
            {
                let wait_status =
                    ffi::VTDecompressionSessionWaitForAsynchronousFrames(self.session);
                if wait_status != 0 {
                    return Err(DecodeError::VideoToolbox(format!(
                        "VTDecompressionSessionWaitForAsynchronousFrames failed: {wait_status}"
                    )));
                }
            }

            if let Some(err) = self.shared.take_error() {
                return Err(err);
            }

            Ok(self.shared.take_frames())
        }
    }

    pub fn flush(&mut self) -> Result<Vec<Nv12Frame>, DecodeError> {
        unsafe {
            let wait_status = ffi::VTDecompressionSessionWaitForAsynchronousFrames(self.session);
            if wait_status != 0 {
                return Err(DecodeError::VideoToolbox(format!(
                    "VTDecompressionSessionWaitForAsynchronousFrames flush failed: {wait_status}"
                )));
            }
            if let Some(err) = self.shared.take_error() {
                return Err(err);
            }
            Ok(self.shared.take_frames())
        }
    }
}

impl Drop for HevcDecoder {
    fn drop(&mut self) {
        unsafe {
            if !self.session.is_null() {
                let _ = ffi::VTDecompressionSessionWaitForAsynchronousFrames(self.session);
                ffi::VTDecompressionSessionInvalidate(self.session);
                ffi::CFRelease(self.session as ffi::CFTypeRef);
                self.session = std::ptr::null_mut();
            }
            if !self.format_desc.is_null() {
                ffi::CFRelease(self.format_desc as ffi::CFTypeRef);
                self.format_desc = std::ptr::null();
            }
            if !self.raw_context.is_null() {
                drop(Arc::from_raw(self.raw_context as *mut SharedState));
                self.raw_context = std::ptr::null_mut();
            }
        }
    }
}

fn create_hevc_format_description(
    extradata: &[u8],
) -> Result<ffi::CMVideoFormatDescriptionRef, DecodeError> {
    if extradata.is_empty() {
        return Err(DecodeError::MissingParameterSets);
    }
    let nalus = crate::parse_length_prefixed_nalus(extradata)?;
    let mut vps: Option<&[u8]> = None;
    let mut sps: Option<&[u8]> = None;
    let mut pps: Option<&[u8]> = None;

    for nalu in &nalus {
        match nalu.nal_type {
            32 if vps.is_none() => vps = Some(nalu.data),
            33 if sps.is_none() => sps = Some(nalu.data),
            34 if pps.is_none() => pps = Some(nalu.data),
            _ => {}
        }
    }

    let (vps, sps, pps) = match (vps, sps, pps) {
        (Some(v), Some(s), Some(p)) => (v, s, p),
        _ => return Err(DecodeError::MissingParameterSets),
    };

    let pt_ptrs: [*const u8; 3] = [vps.as_ptr(), sps.as_ptr(), pps.as_ptr()];
    let pt_sizes: [usize; 3] = [vps.len(), sps.len(), pps.len()];

    let mut format_desc: ffi::CMVideoFormatDescriptionRef = std::ptr::null();
    let status = unsafe {
        ffi::CMVideoFormatDescriptionCreateFromHEVCParameterSets(
            std::ptr::null(),
            3,
            pt_ptrs.as_ptr(),
            pt_sizes.as_ptr(),
            NAL_LENGTH_BYTES as i32,
            std::ptr::null(),
            &mut format_desc,
        )
    };

    if status != 0 || format_desc.is_null() {
        return Err(DecodeError::VideoToolboxInit(format!(
            "CMVideoFormatDescriptionCreateFromHEVCParameterSets failed: {status}"
        )));
    }

    Ok(format_desc)
}

fn create_h264_format_description(
    extradata: &[u8],
) -> Result<ffi::CMVideoFormatDescriptionRef, DecodeError> {
    if extradata.is_empty() {
        return Err(DecodeError::MissingParameterSets);
    }
    let nalus = crate::parse_length_prefixed_nalus(extradata)?;
    let mut sps: Option<&[u8]> = None;
    let mut pps: Option<&[u8]> = None;

    for nalu in &nalus {
        let h264_type = nalu.data.first().map(|b| b & 0x1f).unwrap_or(0);
        match h264_type {
            7 if sps.is_none() => sps = Some(nalu.data),
            8 if pps.is_none() => pps = Some(nalu.data),
            _ => {}
        }
    }

    let (sps, pps) = match (sps, pps) {
        (Some(s), Some(p)) => (s, p),
        _ => return Err(DecodeError::MissingParameterSets),
    };

    let pt_ptrs: [*const u8; 2] = [sps.as_ptr(), pps.as_ptr()];
    let pt_sizes: [usize; 2] = [sps.len(), pps.len()];

    let mut format_desc: ffi::CMVideoFormatDescriptionRef = std::ptr::null();
    let status = unsafe {
        ffi::CMVideoFormatDescriptionCreateFromH264ParameterSets(
            std::ptr::null(),
            2,
            pt_ptrs.as_ptr(),
            pt_sizes.as_ptr(),
            NAL_LENGTH_BYTES as i32,
            &mut format_desc,
        )
    };

    if status != 0 || format_desc.is_null() {
        return Err(DecodeError::VideoToolboxInit(format!(
            "CMVideoFormatDescriptionCreateFromH264ParameterSets failed: {status}"
        )));
    }

    Ok(format_desc)
}

unsafe extern "C" fn decompression_output_callback(
    decompression_output_refcon: *mut c_void,
    source_frame_refcon: *mut c_void,
    status: ffi::OSStatus,
    info_flags: ffi::VTDecodeInfoFlags,
    image_buffer: ffi::CVImageBufferRef,
    presentation_time_stamp: ffi::CMTime,
    _presentation_duration: ffi::CMTime,
) {
    if decompression_output_refcon.is_null() {
        return;
    }
    let shared = &*(decompression_output_refcon as *const SharedState);
    shared.on_frame(
        source_frame_refcon,
        status,
        info_flags,
        image_buffer,
        presentation_time_stamp,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vt_new_with_missing_parameter_sets_fails() {
        assert!(matches!(
            HevcDecoder::new(&[]),
            Err(DecodeError::MissingParameterSets)
        ));
        let incomplete = [0, 0, 0, 2, 0x42, 0x01];
        assert!(matches!(
            HevcDecoder::new(&incomplete),
            Err(DecodeError::MissingParameterSets)
        ));
    }

    #[test]
    fn vt_new_h264_with_missing_parameter_sets_fails() {
        assert!(matches!(
            HevcDecoder::new_h264(&[]),
            Err(DecodeError::MissingParameterSets)
        ));
        let sps_only = [0, 0, 0, 2, 0x67, 0x42];
        assert!(matches!(
            HevcDecoder::new_h264(&sps_only),
            Err(DecodeError::MissingParameterSets)
        ));
    }

    #[test]
    fn vt_decode_malformed_access_unit_returns_typed_error() {
        let access_unit: Vec<_> = include_str!("../../tests/fixtures/black_16x16.hevc.hex")
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect();
        let mut decoder = HevcDecoder::new(&access_unit).unwrap();

        assert!(matches!(
            decoder.decode(&[0, 0, 0], 1),
            Err(DecodeError::TruncatedNalu { offset: 0 })
        ));

        assert!(matches!(
            decoder.decode(&[0, 0, 0, 0], 1),
            Err(DecodeError::EmptyNalu { offset: 0 })
        ));
    }

    #[test]
    fn vt_decode_black_16x16_frame() {
        let access_unit: Vec<_> = include_str!("../../tests/fixtures/black_16x16.hevc.hex")
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect();
        let (kind, mut decoder) = HevcDecoder::from_keyframe_auto(&access_unit).unwrap();
        assert_eq!(kind, crate::CodecKind::Hevc);
        assert_eq!(decoder.acceleration(), HardwareAcceleration::VideoToolbox);

        let frames = decoder.decode(&access_unit, 42).unwrap();
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        assert_eq!(frame.width, 16);
        assert_eq!(frame.height, 16);
        assert_eq!(frame.timestamp_ms, 42);
        assert_eq!(frame.y_stride, 16);
        assert_eq!(frame.uv_stride, 16);
        assert_eq!(frame.y_plane.len(), 16 * 16);
        assert_eq!(frame.uv_plane.len(), 16 * 8);

        for &y in &frame.y_plane {
            assert!(y <= 32);
        }
        for &uv in &frame.uv_plane {
            assert!(uv >= 110 && uv <= 146);
        }

        let frames2 = decoder.decode(&access_unit, 43).unwrap();
        assert_eq!(frames2.len(), 1);
        assert_eq!(frames2[0].timestamp_ms, 43);

        let flushed = decoder.flush().unwrap();
        assert!(flushed.is_empty() || flushed[0].width == 16);
    }
}
