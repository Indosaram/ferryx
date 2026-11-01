//! NVIDIA DMA-BUF import using FFmpeg 7's supported DRM -> Vulkan -> CUDA route.
//! Sources: FFmpeg n7.1 libavutil/hwcontext_cuda.c (cuda_device_derive only
//! accepts Vulkan), hwcontext_vulkan.c (vulkan_map_from_drm and
//! vulkan_transfer_data_to_cuda). The last step is a device-to-device copy,
//! NOT a zero-copy CUDA mapping. No host pixel download/upload occurs.
use super::capture_linux_dmabuf::DmabufCapturedFrame;
use crate::encode_linux::{EncoderConfig, HwBufferRef};
use ffmpeg_next::{self as ffmpeg, ffi, format::Pixel, frame};
use std::{ffi::CString, os::fd::AsRawFd, ptr};

pub fn render_node() -> String {
    std::env::var("MAHO_VAAPI_DEVICE").unwrap_or_else(|_| "/dev/dri/renderD128".into())
}

pub fn nvidia_driver(driver: &str) -> bool {
    driver == "nvidia"
}

pub fn is_nvidia(node: &str) -> Result<bool, String> {
    use std::os::unix::fs::MetadataExt;
    let dev = std::fs::metadata(node).map_err(|e| e.to_string())?.rdev();
    let driver = std::fs::read_link(format!(
        "/sys/dev/char/{}:{}/device/driver",
        libc::major(dev),
        libc::minor(dev)
    ))
    .map_err(|e| format!("render-node driver: {e}"))?;
    Ok(nvidia_driver(
        driver
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default(),
    ))
}

fn check(status: i32) -> Result<(), String> {
    if status < 0 {
        Err(ffmpeg::Error::from(status).to_string())
    } else {
        Ok(())
    }
}

unsafe fn owned(raw: *mut ffi::AVBufferRef) -> Result<HwBufferRef, String> {
    if raw.is_null() {
        return Err("hardware context allocation failed".into());
    }
    let result = unsafe { HwBufferRef::retain(raw) }.map_err(|e| e.to_string());
    let mut raw = raw;
    unsafe {
        ffi::av_buffer_unref(&mut raw);
    }
    result
}

fn device(kind: ffi::AVHWDeviceType, name: &str) -> Result<HwBufferRef, String> {
    let name = CString::new(name).map_err(|e| e.to_string())?;
    let mut raw = ptr::null_mut();
    check(unsafe {
        ffi::av_hwdevice_ctx_create(&mut raw, kind, name.as_ptr(), ptr::null_mut(), 0)
    })?;
    unsafe { owned(raw) }
}

pub fn frame_parameters(
    width: u32,
    height: u32,
    hardware: Pixel,
    software: Pixel,
) -> Result<(i32, i32, Pixel, Pixel), String> {
    if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
        return Err("invalid hardware frame geometry".into());
    }
    Ok((width as i32, height as i32, hardware, software))
}

fn frames(
    device: &HwBufferRef,
    width: u32,
    height: u32,
    hardware: Pixel,
    software: Pixel,
) -> Result<HwBufferRef, String> {
    let (width, height, hardware, software) = frame_parameters(width, height, hardware, software)?;
    let result = unsafe { owned(ffi::av_hwframe_ctx_alloc(device.as_ptr())) }?;
    unsafe {
        let ctx = (*result.as_ptr()).data.cast::<ffi::AVHWFramesContext>();
        (*ctx).width = width;
        (*ctx).height = height;
        (*ctx).format = hardware.into();
        (*ctx).sw_format = software.into();
        check(ffi::av_hwframe_ctx_init(result.as_ptr()))?;
    }
    Ok(result)
}

// ABI from libavutil/hwcontext_drm.h, which ffmpeg-sys does not include.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Object {
    fd: i32,
    size: usize,
    modifier: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Plane {
    object_index: i32,
    offset: isize,
    pitch: isize,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Layer {
    format: u32,
    nb_planes: i32,
    planes: [Plane; 4],
}
#[repr(C)]
#[derive(Default)]
struct Descriptor {
    nb_objects: i32,
    objects: [Object; 4],
    nb_layers: i32,
    layers: [Layer; 4],
}

unsafe extern "C" fn free_descriptor(_: *mut std::ffi::c_void, data: *mut u8) {
    let descriptor = unsafe { Box::from_raw(data.cast::<Descriptor>()) };
    unsafe {
        libc::close(descriptor.objects[0].fd);
    }
}

pub struct CudaPipeline {
    cuda_frames: HwBufferRef,
    cuda: HwBufferRef,
    vulkan: HwBufferRef,
    drm: HwBufferRef,
    input: Option<(u32, u32, u32, bool)>,
    drm_frames: Option<HwBufferRef>,
    vk_frames: Option<HwBufferRef>,
    graph: Option<ffmpeg::filter::Graph>,
    config: EncoderConfig,
}

impl CudaPipeline {
    fn synchronize(&self) -> Result<(), String> {
        #[repr(C)]
        struct CudaDevice {
            context: *mut std::ffi::c_void,
            stream: *mut std::ffi::c_void,
        }
        unsafe {
            let lib = libloading::Library::new("libcuda.so.1").map_err(|e| e.to_string())?;
            let push: libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void) -> i32> = lib
                .get(b"cuCtxPushCurrent_v2\0")
                .map_err(|e| e.to_string())?;
            let pop: libloading::Symbol<unsafe extern "C" fn(*mut *mut std::ffi::c_void) -> i32> =
                lib.get(b"cuCtxPopCurrent_v2\0")
                    .map_err(|e| e.to_string())?;
            let sync: libloading::Symbol<unsafe extern "C" fn(*mut std::ffi::c_void) -> i32> = lib
                .get(b"cuStreamSynchronize\0")
                .map_err(|e| e.to_string())?;
            let device = &*(*(*self.cuda.as_ptr()).data.cast::<ffi::AVHWDeviceContext>())
                .hwctx
                .cast::<CudaDevice>();
            let status = push(device.context);
            if status != 0 {
                return Err(format!("CUDA context push failed: {status}"));
            }
            let status = sync(device.stream);
            let mut previous = ptr::null_mut();
            let pop_status = pop(&mut previous);
            if status != 0 || pop_status != 0 {
                return Err(format!(
                    "CUDA synchronize/pop failed: {status}/{pop_status}"
                ));
            }
        }
        Ok(())
    }
    pub fn new(config: EncoderConfig) -> Result<Self, String> {
        ffmpeg::init().map_err(|e| e.to_string())?;
        let drm = device(ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_DRM, &render_node())?;
        let mut vk = ptr::null_mut();
        let mut options = ffmpeg::Dictionary::new();
        options.set("disable_multiplane", "1");
        check(unsafe {
            ffi::av_hwdevice_ctx_create_derived_opts(
                &mut vk,
                ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VULKAN,
                drm.as_ptr(),
                options.as_mut_ptr(),
                0,
            )
        })?;
        let vulkan = unsafe { owned(vk) }?;
        let mut cu = ptr::null_mut();
        check(unsafe {
            ffi::av_hwdevice_ctx_create_derived(
                &mut cu,
                ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
                vulkan.as_ptr(),
                0,
            )
        })?;
        let cuda = unsafe { owned(cu) }?;
        let cuda_frames = frames(&cuda, config.width, config.height, Pixel::CUDA, Pixel::NV12)?;
        Ok(Self {
            cuda_frames,
            cuda,
            vulkan,
            drm,
            input: None,
            drm_frames: None,
            vk_frames: None,
            graph: None,
            config,
        })
    }

    pub fn frames(&self) -> *mut ffi::AVBufferRef {
        self.cuda_frames.as_ptr()
    }

    fn configure(&mut self, source: &DmabufCapturedFrame) -> Result<(), String> {
        let key = (
            source.offer.width,
            source.offer.height,
            source.offer.format,
            source.y_inverted,
        );
        if self.input == Some(key) {
            return Ok(());
        }
        let sw = match source.offer.format {
            0x34325258 => Pixel::BGRZ, // DRM_FORMAT_XRGB8888
            0x34325241 => Pixel::BGRA, // DRM_FORMAT_ARGB8888
            0x34324258 => Pixel::RGBZ,
            0x34324241 => Pixel::RGBA,
            _ => return Err("unsupported NVIDIA capture fourcc".into()),
        };
        let drm_frames = frames(&self.drm, key.0, key.1, Pixel::DRM_PRIME, sw)?;
        let vk_frames = frames(&self.vulkan, key.0, key.1, Pixel::VULKAN, sw)?;
        let mut graph = ffmpeg::filter::Graph::new();
        let args = format!(
            "video_size={}x{}:pix_fmt={}:time_base=1/{}:pixel_aspect=1/1",
            key.0,
            key.1,
            ffi::AVPixelFormat::from(Pixel::VULKAN) as i32,
            self.config.fps
        );
        let mut input = graph
            .add(
                &ffmpeg::filter::find("buffer").ok_or("FFmpeg buffer filter unavailable")?,
                "in",
                &args,
            )
            .map_err(|e| e.to_string())?;
        unsafe {
            let params = ffi::av_buffersrc_parameters_alloc();
            if params.is_null() {
                return Err("buffer source allocation failed".into());
            }
            (*params).hw_frames_ctx = vk_frames.as_ptr();
            let status = ffi::av_buffersrc_parameters_set(input.as_mut_ptr(), params);
            ffi::av_free(params.cast());
            check(status)?;
        }
        graph
            .add(
                &ffmpeg::filter::find("buffersink").ok_or("FFmpeg buffersink unavailable")?,
                "out",
                "",
            )
            .map_err(|e| e.to_string())?;
        let flip = if key.3 { "vflip_vulkan," } else { "" };
        graph
            .output("in", 0)
            .map_err(|e| e.to_string())?
            .input("out", 0)
            .map_err(|e| e.to_string())?
            .parse(&format!(
                "{flip}scale_vulkan=w={}:h={}:format=nv12:out_range=full",
                self.config.width, self.config.height
            ))
            .map_err(|e| e.to_string())?;
        // Filters need the same physical device and external-memory extensions.
        unsafe {
            let raw = graph.as_mut_ptr();
            for i in 0..(*raw).nb_filters {
                let filter = *(*raw).filters.add(i as usize);
                (*filter).hw_device_ctx = ffi::av_buffer_ref(self.vulkan.as_ptr());
            }
        }
        graph.validate().map_err(|e| e.to_string())?;
        self.graph = Some(graph);
        self.drm_frames = Some(drm_frames);
        self.vk_frames = Some(vk_frames);
        self.input = Some(key);
        Ok(())
    }

    pub fn import(&mut self, source: &DmabufCapturedFrame) -> Result<frame::Video, String> {
        self.configure(source)?;
        let fd = unsafe { libc::fcntl(source.buffer.fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let size = unsafe { libc::lseek(fd, 0, libc::SEEK_END) };
        if size < 0 {
            unsafe {
                libc::close(fd);
            }
            return Err(format!(
                "DMA-BUF size query: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut desc = Box::<Descriptor>::default();
        desc.nb_objects = 1;
        desc.objects[0] = Object {
            fd,
            size: size as usize,
            modifier: source.buffer.modifier,
        };
        desc.nb_layers = 1;
        desc.layers[0].format = source.offer.format;
        desc.layers[0].nb_planes = 1;
        desc.layers[0].planes[0] = Plane {
            object_index: 0,
            offset: source.buffer.offset as isize,
            pitch: source.buffer.stride as isize,
        };
        let data = Box::into_raw(desc).cast::<u8>();
        let mut drm_frame = frame::Video::empty();
        let mut mapped = frame::Video::empty();
        unsafe {
            let raw = drm_frame.as_mut_ptr();
            (*raw).buf[0] = ffi::av_buffer_create(
                data,
                std::mem::size_of::<Descriptor>(),
                Some(free_descriptor),
                ptr::null_mut(),
                0,
            );
            if (*raw).buf[0].is_null() {
                free_descriptor(ptr::null_mut(), data);
                return Err("DRM descriptor buffer allocation failed".into());
            }
            (*raw).data[0] = data;
            (*raw).format = ffi::AVPixelFormat::from(Pixel::DRM_PRIME) as i32;
            (*raw).width = source.offer.width as i32;
            (*raw).height = source.offer.height as i32;
            (*raw).hw_frames_ctx = ffi::av_buffer_ref(self.drm_frames.as_ref().unwrap().as_ptr());
            (*mapped.as_mut_ptr()).format = ffi::AVPixelFormat::from(Pixel::VULKAN) as i32;
            (*mapped.as_mut_ptr()).hw_frames_ctx =
                ffi::av_buffer_ref(self.vk_frames.as_ref().unwrap().as_ptr());
            check(ffi::av_hwframe_map(
                mapped.as_mut_ptr(),
                raw,
                ffi::AV_HWFRAME_MAP_READ as i32,
            ))?;
        }
        let graph = self.graph.as_mut().unwrap();
        graph
            .get("in")
            .unwrap()
            .source()
            .add(&mapped)
            .map_err(|e| e.to_string())?;
        let mut converted = frame::Video::empty();
        graph
            .get("out")
            .unwrap()
            .sink()
            .frame(&mut converted)
            .map_err(|e| e.to_string())?;
        let mut output = frame::Video::empty();
        unsafe {
            check(ffi::av_hwframe_get_buffer(
                self.cuda_frames.as_ptr(),
                output.as_mut_ptr(),
                0,
            ))?;
            check(ffi::av_hwframe_transfer_data(
                output.as_mut_ptr(),
                converted.as_ptr(),
                0,
            ))?;
        }
        // FFmpeg's Vulkan/CUDA transfer is asynchronous. Wait on its exact CUDA
        // stream before releasing the capture lease or letting NVENC read pixels.
        self.synchronize()?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapter_driver_selection_is_exact() {
        assert!(nvidia_driver("nvidia"));
        for driver in ["amdgpu", "i915", "xe", "nouveau", "nvidia-drm", ""] {
            assert!(!nvidia_driver(driver));
        }
    }
    #[test]
    fn context_parameters_preserve_hardware_software_and_geometry() {
        assert_eq!(
            frame_parameters(1920, 1080, Pixel::CUDA, Pixel::NV12).unwrap(),
            (1920, 1080, Pixel::CUDA, Pixel::NV12)
        );
        assert!(frame_parameters(0, 1080, Pixel::VULKAN, Pixel::BGRA).is_err());
        assert!(frame_parameters(u32::MAX, 1080, Pixel::CUDA, Pixel::NV12).is_err());
    }
}
