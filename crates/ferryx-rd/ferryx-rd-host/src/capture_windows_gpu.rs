use std::mem::ManuallyDrop;
use std::ptr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::capture_windows::CaptureError;
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, LUID, RECT};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1,
    D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_9_1, D3D_FEATURE_LEVEL_9_2,
    D3D_FEATURE_LEVEL_9_3,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Resource,
    ID3D11Texture2D, ID3D11VideoContext1, ID3D11VideoDevice, ID3D11VideoProcessor,
    ID3D11VideoProcessorEnumerator, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_BIND_VIDEO_ENCODER, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
    D3D11_SDK_VERSION, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_OPTIMAL_SPEED, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_FULL_G22_LEFT_P601,
    DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication,
    IDXGIResource, DXGI_ERROR_ACCESS_DENIED, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_NOT_FOUND,
    DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, DXGI_OUTPUT_DESC,
};

#[derive(Clone)]
pub struct GpuFrame {
    pub texture: Arc<ID3D11Texture2D>,
    pub captured_at: Instant,
    pub capture_id: u64,
}

// SAFETY: ID3D11Texture2D COM interface is thread-safe on D3D11 devices configured
// with ID3D11Multithread::SetMultithreadProtected(true).
unsafe impl Send for GpuFrame {}
// SAFETY: Immutable concurrent references to ID3D11Texture2D across threads are valid
// with multithread protection enabled.
unsafe impl Sync for GpuFrame {}

pub struct AcquiredMetadata {}

pub struct GpuCapture {
    adapter_name: String,
    adapter_luid: LUID,
    d3d11_device: ID3D11Device,
    _d3d11_context: ID3D11DeviceContext,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext1,
    video_processor: ID3D11VideoProcessor,
    vp_enumerator: ID3D11VideoProcessorEnumerator,
    duplication: IDXGIOutputDuplication,
    pool: [Arc<ID3D11Texture2D>; 4],
    width: u32,
    height: u32,
}

impl GpuCapture {
    pub fn new_scaled(
        display_index: usize,
        expected_width: u32,
        expected_height: u32,
        target_width: u32,
        target_height: u32,
    ) -> Result<Self, CaptureError> {
        let initialization =
            |error: windows::core::Error| CaptureError::Initialization(error.to_string());

        // SAFETY: CreateDXGIFactory1 is safe to call for enumerating DXGI hardware adapters.
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.map_err(initialization)?;
        let mut attached_index = 0;
        let mut matched_adapter = None;
        let mut matched_output = None;
        let mut adapter_luid = LUID::default();
        let mut adapter_name = String::new();

        for adapter_idx in 0.. {
            // SAFETY: EnumAdapters1 returns DXGI_ERROR_NOT_FOUND when index exceeds count.
            let adapter: IDXGIAdapter1 = match unsafe { factory.EnumAdapters1(adapter_idx) } {
                Ok(a) => a,
                Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
                Err(error) => return Err(initialization(error)),
            };
            for output_idx in 0.. {
                // SAFETY: EnumOutputs returns error when index exceeds available outputs.
                let output = match unsafe { adapter.EnumOutputs(output_idx) } {
                    Ok(o) => o,
                    Err(_) => break,
                };
                // SAFETY: GetDesc populates DXGI_OUTPUT_DESC.
                let desc: DXGI_OUTPUT_DESC = unsafe { output.GetDesc() }.map_err(initialization)?;
                if !desc.AttachedToDesktop.as_bool() {
                    continue;
                }
                if attached_index != display_index {
                    attached_index += 1;
                    continue;
                }
                // SAFETY: GetDesc1 populates DXGI_ADAPTER_DESC1 with LUID and name.
                let adapter_desc = unsafe { adapter.GetDesc1() }.map_err(initialization)?;
                adapter_luid = adapter_desc.AdapterLuid;
                let null_pos = adapter_desc
                    .Description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(adapter_desc.Description.len());
                adapter_name = String::from_utf16_lossy(&adapter_desc.Description[..null_pos]);
                matched_adapter = Some(adapter);
                matched_output = Some(output);
                break;
            }
            if matched_output.is_some() {
                break;
            }
        }

        let adapter = matched_adapter.ok_or_else(|| {
            CaptureError::Initialization("No suitable output display was found".into())
        })?;
        let output = matched_output.ok_or_else(|| {
            CaptureError::Initialization("No suitable output display was found".into())
        })?;

        let mut device = None;
        let mut context = None;
        let mut feature_level = D3D_FEATURE_LEVEL_9_1;
        let feature_levels = [
            D3D_FEATURE_LEVEL_11_1,
            D3D_FEATURE_LEVEL_11_0,
            D3D_FEATURE_LEVEL_10_1,
            D3D_FEATURE_LEVEL_10_0,
            D3D_FEATURE_LEVEL_9_3,
            D3D_FEATURE_LEVEL_9_2,
            D3D_FEATURE_LEVEL_9_1,
        ];
        // SAFETY: D3D11CreateDevice creates a Direct3D 11 device and immediate context.
        unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut feature_level),
                Some(&mut context),
            )
            .map_err(initialization)?;
        }
        let device: ID3D11Device = device.ok_or_else(|| {
            CaptureError::Initialization("D3D11CreateDevice returned null device".into())
        })?;
        let context: ID3D11DeviceContext = context.ok_or_else(|| {
            CaptureError::Initialization("D3D11CreateDevice returned null context".into())
        })?;

        let multithread: ID3D11Multithread = device.cast().map_err(initialization)?;
        // SAFETY: SetMultithreadProtected(true) enables thread safety across Direct3D calls.
        if let Err(error) = unsafe { multithread.SetMultithreadProtected(true) }.ok() {
            tracing::debug!(
                %error,
                "D3D11 multithread protection was not previously enabled"
            );
        }

        let video_device: ID3D11VideoDevice = device.cast().map_err(initialization)?;
        let video_context: ID3D11VideoContext1 = context.cast().map_err(initialization)?;

        let output1: IDXGIOutput1 = output.cast().map_err(initialization)?;
        // SAFETY: DuplicateOutput enables desktop duplication for output1.
        let duplication = unsafe { output1.DuplicateOutput(&device) }.map_err(initialization)?;
        // SAFETY: GetDesc populates DXGI_OUTDUPL_DESC with mode dimensions.
        let dupl_desc = unsafe { duplication.GetDesc() };
        let width = dupl_desc.ModeDesc.Width;
        let height = dupl_desc.ModeDesc.Height;

        if (expected_width > 0 && width != expected_width)
            || (expected_height > 0 && height != expected_height)
        {
            tracing::warn!(
                dupl_width = width,
                dupl_height = height,
                expected_width,
                expected_height,
                "DXGI duplication dimensions differ from expected"
            );
        }

        let output_width = if target_width == 0 {
            width
        } else {
            target_width
        };
        let output_height = if target_height == 0 {
            height
        } else {
            target_height
        };
        if output_width % 2 != 0 || output_height % 2 != 0 {
            return Err(CaptureError::Initialization(
                "NV12 output dimensions must be even".into(),
            ));
        }

        let content_desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            InputWidth: width,
            InputHeight: height,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: 60,
                Denominator: 1,
            },
            OutputWidth: output_width,
            OutputHeight: output_height,
            Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
        };

        // SAFETY: CreateVideoProcessorEnumerator enumerates video processor capabilities.
        let vp_enumerator = unsafe {
            video_device
                .CreateVideoProcessorEnumerator(&content_desc)
                .map_err(initialization)?
        };

        // SAFETY: CreateVideoProcessor creates video processor for color conversion.
        let video_processor = unsafe {
            video_device
                .CreateVideoProcessor(&vp_enumerator, 0)
                .map_err(initialization)?
        };

        // SAFETY: Configuring video processor color space and stream format.
        unsafe {
            video_context.VideoProcessorSetStreamColorSpace1(
                &video_processor,
                0,
                DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
            );
            video_context.VideoProcessorSetOutputColorSpace1(
                &video_processor,
                DXGI_COLOR_SPACE_YCBCR_FULL_G22_LEFT_P601,
            );
            video_context.VideoProcessorSetStreamAutoProcessingMode(&video_processor, 0, false);
            video_context.VideoProcessorSetStreamFrameFormat(
                &video_processor,
                0,
                D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            );
            let (source_rect, rect) =
                processor_rects((width, height), (output_width, output_height));
            video_context.VideoProcessorSetStreamSourceRect(
                &video_processor,
                0,
                true,
                Some(&source_rect),
            );
            video_context.VideoProcessorSetStreamDestRect(&video_processor, 0, true, Some(&rect));
            video_context.VideoProcessorSetOutputTargetRect(&video_processor, true, Some(&rect));
        }

        let desc = D3D11_TEXTURE2D_DESC {
            Width: output_width,
            Height: output_height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_VIDEO_ENCODER.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };

        let mut pool_vec = Vec::with_capacity(4);
        for _ in 0..4 {
            let mut tex = None;
            // SAFETY: CreateTexture2D creates an NV12 texture with encoder bind flags.
            let res = unsafe { device.CreateTexture2D(&desc, None, Some(&mut tex)) };
            let tex = match res {
                Ok(()) => tex.unwrap(),
                Err(_) => {
                    let mut fallback_desc = desc;
                    fallback_desc.BindFlags =
                        (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32;
                    let mut fallback_tex = None;
                    // SAFETY: Fallback CreateTexture2D without video encoder bind flag.
                    unsafe {
                        device
                            .CreateTexture2D(&fallback_desc, None, Some(&mut fallback_tex))
                            .map_err(initialization)?;
                    }
                    fallback_tex.unwrap()
                }
            };
            pool_vec.push(Arc::new(tex));
        }
        let pool: [Arc<ID3D11Texture2D>; 4] = pool_vec
            .try_into()
            .map_err(|_| CaptureError::Initialization("pool sizing failed".into()))?;

        Ok(Self {
            adapter_name,
            adapter_luid,
            d3d11_device: device,
            _d3d11_context: context,
            video_device,
            video_context,
            video_processor,
            vp_enumerator,
            duplication,
            pool,
            width: output_width,
            height: output_height,
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn adapter_luid(&self) -> LUID {
        self.adapter_luid
    }

    pub fn d3d11_device(&self) -> ID3D11Device {
        self.d3d11_device.clone()
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn texture(&self, slot: usize) -> Arc<ID3D11Texture2D> {
        self.pool[slot % 4].clone()
    }

    pub fn available_slot(&self) -> Option<usize> {
        crate::windows_logic::available_texture_slot(&self.pool)
    }

    pub fn acquire_and_blt(
        &mut self,
        timeout: Duration,
        slot: usize,
    ) -> Result<AcquiredMetadata, CaptureError> {
        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);

        // SAFETY: AcquireNextFrame queries the next frame metadata and DXGI resource pointer.
        let hr = unsafe {
            self.duplication
                .AcquireNextFrame(timeout_ms, &mut frame_info, &mut resource)
        };
        if let Err(error) = hr {
            match error.code() {
                DXGI_ERROR_WAIT_TIMEOUT => return Err(CaptureError::Timeout),
                DXGI_ERROR_ACCESS_LOST => return Err(CaptureError::AccessLost),
                DXGI_ERROR_ACCESS_DENIED => return Err(CaptureError::AccessDenied),
                _ => return Err(CaptureError::Capture(error.to_string())),
            }
        }

        let Some(resource) = resource else {
            // SAFETY: ReleaseFrame frees acquired desktop buffer back to DXGI.
            let _ = unsafe { self.duplication.ReleaseFrame() };
            return Err(CaptureError::Timeout);
        };

        if frame_info.LastPresentTime == 0 {
            // SAFETY: ReleaseFrame frees acquired desktop buffer back to DXGI.
            let _ = unsafe { self.duplication.ReleaseFrame() };
            return Err(CaptureError::Timeout);
        }

        let src_texture: ID3D11Texture2D = resource
            .cast()
            .map_err(|e| CaptureError::Capture(format!("resource cast: {e}")))?;

        let dst_texture = &self.pool[slot % 4];
        let blt_res = self.blt(&src_texture, dst_texture);

        // SAFETY: ReleaseFrame frees acquired desktop buffer back to DXGI.
        let _ = unsafe { self.duplication.ReleaseFrame() };
        blt_res.map_err(|e| CaptureError::Capture(format!("vp blt: {e}")))?;

        Ok(AcquiredMetadata {})
    }

    fn blt(&self, src: &ID3D11Texture2D, dst: &ID3D11Texture2D) -> windows::core::Result<()> {
        let mut ivd = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        ivd.Anonymous.Texture2D = D3D11_TEX2D_VPIV {
            MipSlice: 0,
            ArraySlice: 0,
        };

        let src_resource: ID3D11Resource = src.cast()?;
        let dst_resource: ID3D11Resource = dst.cast()?;

        let mut iv = None;
        // SAFETY: CreateVideoProcessorInputView wraps src_resource as input view.
        unsafe {
            self.video_device.CreateVideoProcessorInputView(
                &src_resource,
                &self.vp_enumerator,
                &ivd,
                Some(&mut iv),
            )?;
        }
        let iv = iv.unwrap();

        let mut ovd = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        ovd.Anonymous.Texture2D = D3D11_TEX2D_VPOV { MipSlice: 0 };

        let mut ov = None;
        // SAFETY: CreateVideoProcessorOutputView wraps dst_resource as output view.
        unsafe {
            self.video_device.CreateVideoProcessorOutputView(
                &dst_resource,
                &self.vp_enumerator,
                &ovd,
                Some(&mut ov),
            )?;
        }
        let ov = ov.unwrap();

        let mut streams = [D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: ptr::null_mut(),
            pInputSurface: ManuallyDrop::new(Some(iv)),
            ppFutureSurfaces: ptr::null_mut(),
            ppPastSurfacesRight: ptr::null_mut(),
            pInputSurfaceRight: ManuallyDrop::new(None),
            ppFutureSurfacesRight: ptr::null_mut(),
        }];

        // SAFETY: VideoProcessorBlt executes hardware color conversion blit.
        let blt_res = unsafe {
            self.video_context
                .VideoProcessorBlt(&self.video_processor, &ov, 0, &streams)
        };

        // SAFETY: the stream owns these view references; VideoProcessorBlt only borrows them.
        unsafe {
            ManuallyDrop::drop(&mut streams[0].pInputSurface);
            ManuallyDrop::drop(&mut streams[0].pInputSurfaceRight);
        }

        blt_res?;
        Ok(())
    }
}

fn processor_rects(source: (u32, u32), target: (u32, u32)) -> (RECT, RECT) {
    let rect = |(width, height)| RECT {
        left: 0,
        top: 0,
        right: width as i32,
        bottom: height as i32,
    };
    (rect(source), rect(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_rectangles_keep_source_native() {
        let (source, target) = processor_rects((1920, 1080), (1280, 720));
        assert_eq!((source.right, source.bottom), (1920, 1080));
        assert_eq!((target.right, target.bottom), (1280, 720));
        assert_eq!(
            (source.left, source.top, target.left, target.top),
            (0, 0, 0, 0)
        );
    }
}
