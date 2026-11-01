use std::os::fd::RawFd;

use ffmpeg_next as ffmpeg;
use libloading::{Library, Symbol};
use thiserror::Error;

pub const VA_RT_FORMAT_RGB32: u32 = 0x20000;
pub const VA_FOURCC_BGRA: u32 = 0x41524742;
pub const VA_FOURCC_BGRX: u32 = 0x58524742;

pub const VA_SURFACE_ATTRIB_MEMORY_TYPE: u32 = 6;
pub const VA_SURFACE_ATTRIB_EXTERNAL_BUFFER_DESCRIPTOR: u32 = 7;

pub const VA_GENERIC_VALUE_TYPE_INTEGER: u32 = 1;
pub const VA_GENERIC_VALUE_TYPE_POINTER: u32 = 3;
pub const VA_SURFACE_ATTRIB_SETTABLE: u32 = 0x2;
pub const VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2: u32 = 0x40000000;

pub const VA_PROFILE_NONE: i32 = -1;
pub const VA_ENTRYPOINT_VIDEO_PROC: i32 = 10;
pub const VA_PROC_PIPELINE_PARAMETER_BUFFER_TYPE: u32 = 41;

pub const VA_PROC_COLOR_STANDARD_EXPLICIT: u32 = 13;
pub const VA_SOURCE_RANGE_FULL: u8 = 2;
pub const VA_STATUS_SUCCESS: i32 = 0;
pub const VA_INVALID_ID: u32 = 0xFFFF_FFFF;

#[derive(Debug, Error)]
pub enum VaapiError {
    #[error("failed to load libva: {0}")]
    LibLoading(#[from] libloading::Error),
    #[error("VA error {0}: {1}")]
    VaStatus(i32, String),
    #[error("failed to get VADisplay from FFmpeg hardware context")]
    DisplayUnavailable,
    #[error("FFmpeg error: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union VaGenericValueUnion {
    pub i: i32,
    pub f: f32,
    pub p: *mut std::ffi::c_void,
    pub fn_ptr: *mut std::ffi::c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaGenericValue {
    pub value_type: u32,
    pub value: VaGenericValueUnion,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaSurfaceAttrib {
    pub attrib_type: u32,
    pub flags: u32,
    pub value: VaGenericValue,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaDrmPrimeObject {
    pub fd: RawFd,
    pub size: u32,
    pub drm_format_modifier: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaDrmPrimeLayer {
    pub drm_format: u32,
    pub num_planes: u32,
    pub object_index: [u32; 4],
    pub offset: [u32; 4],
    pub pitch: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VaDrmPrimeSurfaceDescriptor {
    pub fourcc: u32,
    pub width: u32,
    pub height: u32,
    pub num_objects: u32,
    pub objects: [VaDrmPrimeObject; 4],
    pub num_layers: u32,
    pub layers: [VaDrmPrimeLayer; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VaRectangle {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VaProcColorProperties {
    pub chroma_sample_location: u8,
    pub color_range: u8,
    pub colour_primaries: u8,
    pub transfer_characteristics: u8,
    pub matrix_coefficients: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
pub struct VaProcPipelineParameterBuffer {
    pub surface: u32,
    pub surface_region: *const VaRectangle,
    pub surface_color_standard: u32,
    pub output_region: *const VaRectangle,
    pub output_background_color: u32,
    pub output_color_standard: u32,
    pub pipeline_flags: u32,
    pub filter_flags: u32,
    pub filters: *mut u32,
    pub num_filters: u32,
    pub forward_references: *mut u32,
    pub num_forward_references: u32,
    pub backward_references: *mut u32,
    pub num_backward_references: u32,
    pub rotation_state: u32,
    pub blend_state: *const std::ffi::c_void,
    pub mirror_state: u32,
    pub additional_outputs: *mut *mut std::ffi::c_void,
    pub num_additional_outputs: u32,
    pub input_surface_flag: u32,
    pub output_surface_flag: u32,
    pub input_color_properties: VaProcColorProperties,
    pub output_color_properties: VaProcColorProperties,
    pub processing_mode: u32,
    pub output_hdr_metadata: *const std::ffi::c_void,
    pub reserved: [u32; 16],
}

pub struct LibVa {
    _lib: Library,
    error_str: unsafe extern "C" fn(status: i32) -> *const std::ffi::c_char,
    create_surfaces: unsafe extern "C" fn(
        dpy: *mut std::ffi::c_void,
        format: u32,
        width: u32,
        height: u32,
        surfaces: *mut u32,
        num_surfaces: u32,
        attrib_list: *mut VaSurfaceAttrib,
        num_attribs: u32,
    ) -> i32,
    destroy_surfaces: unsafe extern "C" fn(
        dpy: *mut std::ffi::c_void,
        surfaces: *mut u32,
        num_surfaces: u32,
    ) -> i32,
    create_config: unsafe extern "C" fn(
        dpy: *mut std::ffi::c_void,
        profile: i32,
        entrypoint: i32,
        attrib_list: *mut std::ffi::c_void,
        num_attribs: i32,
        config_id: *mut u32,
    ) -> i32,
    destroy_config: unsafe extern "C" fn(dpy: *mut std::ffi::c_void, config_id: u32) -> i32,
    create_context: unsafe extern "C" fn(
        dpy: *mut std::ffi::c_void,
        config_id: u32,
        picture_width: i32,
        picture_height: i32,
        flag: i32,
        render_targets: *mut u32,
        num_render_targets: i32,
        context_id: *mut u32,
    ) -> i32,
    destroy_context: unsafe extern "C" fn(dpy: *mut std::ffi::c_void, context_id: u32) -> i32,
    create_buffer: unsafe extern "C" fn(
        dpy: *mut std::ffi::c_void,
        context_id: u32,
        buffer_type: u32,
        size: u32,
        num_elements: u32,
        data: *mut std::ffi::c_void,
        buffer_id: *mut u32,
    ) -> i32,
    destroy_buffer: unsafe extern "C" fn(dpy: *mut std::ffi::c_void, buffer_id: u32) -> i32,
    begin_picture: unsafe extern "C" fn(
        dpy: *mut std::ffi::c_void,
        context_id: u32,
        render_target: u32,
    ) -> i32,
    render_picture: unsafe extern "C" fn(
        dpy: *mut std::ffi::c_void,
        context_id: u32,
        buffers: *mut u32,
        num_buffers: i32,
    ) -> i32,
    end_picture: unsafe extern "C" fn(dpy: *mut std::ffi::c_void, context_id: u32) -> i32,
    sync_surface: unsafe extern "C" fn(dpy: *mut std::ffi::c_void, render_target: u32) -> i32,
}

impl LibVa {
    pub fn load() -> Result<Self, VaapiError> {
        // SAFETY: dlopening libva dynamically binds standard libva entry points matching the measured C ABI.
        unsafe {
            let lib = Library::new("libva.so.2").or_else(|_| Library::new("libva.so"))?;
            let error_str: Symbol<unsafe extern "C" fn(i32) -> *const std::ffi::c_char> =
                lib.get(b"vaErrorStr\0")?;
            let create_surfaces: Symbol<
                unsafe extern "C" fn(
                    *mut std::ffi::c_void,
                    u32,
                    u32,
                    u32,
                    *mut u32,
                    u32,
                    *mut VaSurfaceAttrib,
                    u32,
                ) -> i32,
            > = lib.get(b"vaCreateSurfaces\0")?;
            let destroy_surfaces: Symbol<
                unsafe extern "C" fn(*mut std::ffi::c_void, *mut u32, u32) -> i32,
            > = lib.get(b"vaDestroySurfaces\0")?;
            let create_config: Symbol<
                unsafe extern "C" fn(
                    *mut std::ffi::c_void,
                    i32,
                    i32,
                    *mut std::ffi::c_void,
                    i32,
                    *mut u32,
                ) -> i32,
            > = lib.get(b"vaCreateConfig\0")?;
            let destroy_config: Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, u32) -> i32> =
                lib.get(b"vaDestroyConfig\0")?;
            let create_context: Symbol<
                unsafe extern "C" fn(
                    *mut std::ffi::c_void,
                    u32,
                    i32,
                    i32,
                    i32,
                    *mut u32,
                    i32,
                    *mut u32,
                ) -> i32,
            > = lib.get(b"vaCreateContext\0")?;
            let destroy_context: Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, u32) -> i32> =
                lib.get(b"vaDestroyContext\0")?;
            let create_buffer: Symbol<
                unsafe extern "C" fn(
                    *mut std::ffi::c_void,
                    u32,
                    u32,
                    u32,
                    u32,
                    *mut std::ffi::c_void,
                    *mut u32,
                ) -> i32,
            > = lib.get(b"vaCreateBuffer\0")?;
            let destroy_buffer: Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, u32) -> i32> =
                lib.get(b"vaDestroyBuffer\0")?;
            let begin_picture: Symbol<
                unsafe extern "C" fn(*mut std::ffi::c_void, u32, u32) -> i32,
            > = lib.get(b"vaBeginPicture\0")?;
            let render_picture: Symbol<
                unsafe extern "C" fn(*mut std::ffi::c_void, u32, *mut u32, i32) -> i32,
            > = lib.get(b"vaRenderPicture\0")?;
            let end_picture: Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, u32) -> i32> =
                lib.get(b"vaEndPicture\0")?;
            let sync_surface: Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, u32) -> i32> =
                lib.get(b"vaSyncSurface\0")?;

            Ok(Self {
                error_str: *error_str,
                create_surfaces: *create_surfaces,
                destroy_surfaces: *destroy_surfaces,
                create_config: *create_config,
                destroy_config: *destroy_config,
                create_context: *create_context,
                destroy_context: *destroy_context,
                create_buffer: *create_buffer,
                destroy_buffer: *destroy_buffer,
                begin_picture: *begin_picture,
                render_picture: *render_picture,
                end_picture: *end_picture,
                sync_surface: *sync_surface,
                _lib: lib,
            })
        }
    }

    pub fn check(&self, status: i32) -> Result<(), VaapiError> {
        if status == VA_STATUS_SUCCESS {
            Ok(())
        } else {
            // SAFETY: vaErrorStr returns a static C string for any status code.
            let msg = unsafe {
                let ptr = (self.error_str)(status);
                if ptr.is_null() {
                    "unknown".to_string()
                } else {
                    std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
                }
            };
            Err(VaapiError::VaStatus(status, msg))
        }
    }
}

pub struct VaapiPipeline {
    _device: crate::encode_linux::HwBufferRef,
    va: LibVa,
    display: *mut std::ffi::c_void,
    config_id: u32,
    context_id: u32,
    imported_surfaces: Vec<u32>,
    pub width: u32,
    pub height: u32,
}

impl VaapiPipeline {
    pub fn new(
        va: LibVa,
        device_buf: *mut ffmpeg::ffi::AVBufferRef,
        width: u32,
        height: u32,
    ) -> Result<Self, VaapiError> {
        if device_buf.is_null() {
            return Err(VaapiError::DisplayUnavailable);
        }
        // SAFETY: device_buf is a valid live AVBufferRef for AVHWDeviceContext whose first field is VADisplay.
        let display = unsafe {
            let hwctx = (*device_buf).data.cast::<ffmpeg::ffi::AVHWDeviceContext>();
            if hwctx.is_null() || (*hwctx).hwctx.is_null() {
                return Err(VaapiError::DisplayUnavailable);
            }
            *(*hwctx).hwctx.cast::<*mut std::ffi::c_void>()
        };

        let device = unsafe { crate::encode_linux::HwBufferRef::retain(device_buf) }
            .map_err(|_| VaapiError::DisplayUnavailable)?;
        let mut config_id = VA_INVALID_ID;
        // SAFETY: display is a valid VADisplay; parameters specify video processing entry point.
        let status = unsafe {
            (va.create_config)(
                display,
                VA_PROFILE_NONE,
                VA_ENTRYPOINT_VIDEO_PROC,
                std::ptr::null_mut(),
                0,
                &mut config_id,
            )
        };
        va.check(status)?;

        let mut context_id = VA_INVALID_ID;
        // SAFETY: display and config_id are valid; target dimensions match frame size.
        let status = unsafe {
            (va.create_context)(
                display,
                config_id,
                width as i32,
                height as i32,
                0,
                std::ptr::null_mut(),
                0,
                &mut context_id,
            )
        };
        if let Err(e) = va.check(status) {
            // SAFETY: cleaning up allocated config_id on context creation error.
            unsafe { (va.destroy_config)(display, config_id) };
            return Err(e);
        }

        Ok(Self {
            _device: device,
            va,
            display,
            config_id,
            context_id,
            imported_surfaces: Vec::new(),
            width,
            height,
        })
    }

    pub fn import_drm_bo(
        &mut self,
        fd: RawFd,
        stride: u32,
        offset: u32,
        format: u32,
        width: u32,
        height: u32,
    ) -> Result<u32, VaapiError> {
        let fourcc = if format == super::capture_linux_dmabuf::GBM_FORMAT_ARGB8888 {
            VA_FOURCC_BGRA
        } else {
            VA_FOURCC_BGRX
        };

        let mut desc = VaDrmPrimeSurfaceDescriptor {
            fourcc,
            width,
            height,
            num_objects: 1,
            objects: [
                VaDrmPrimeObject {
                    fd,
                    size: stride * height + offset,
                    drm_format_modifier: 0,
                },
                VaDrmPrimeObject {
                    fd: -1,
                    size: 0,
                    drm_format_modifier: 0,
                },
                VaDrmPrimeObject {
                    fd: -1,
                    size: 0,
                    drm_format_modifier: 0,
                },
                VaDrmPrimeObject {
                    fd: -1,
                    size: 0,
                    drm_format_modifier: 0,
                },
            ],
            num_layers: 1,
            layers: [
                VaDrmPrimeLayer {
                    drm_format: format,
                    num_planes: 1,
                    object_index: [0, 0, 0, 0],
                    offset: [offset, 0, 0, 0],
                    pitch: [stride, 0, 0, 0],
                },
                VaDrmPrimeLayer {
                    drm_format: 0,
                    num_planes: 0,
                    object_index: [0; 4],
                    offset: [0; 4],
                    pitch: [0; 4],
                },
                VaDrmPrimeLayer {
                    drm_format: 0,
                    num_planes: 0,
                    object_index: [0; 4],
                    offset: [0; 4],
                    pitch: [0; 4],
                },
                VaDrmPrimeLayer {
                    drm_format: 0,
                    num_planes: 0,
                    object_index: [0; 4],
                    offset: [0; 4],
                    pitch: [0; 4],
                },
            ],
        };

        let mut attribs = [
            VaSurfaceAttrib {
                attrib_type: VA_SURFACE_ATTRIB_MEMORY_TYPE,
                flags: VA_SURFACE_ATTRIB_SETTABLE,
                value: VaGenericValue {
                    value_type: VA_GENERIC_VALUE_TYPE_INTEGER,
                    value: VaGenericValueUnion {
                        i: VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2 as i32,
                    },
                },
            },
            VaSurfaceAttrib {
                attrib_type: VA_SURFACE_ATTRIB_EXTERNAL_BUFFER_DESCRIPTOR,
                flags: VA_SURFACE_ATTRIB_SETTABLE,
                value: VaGenericValue {
                    value_type: VA_GENERIC_VALUE_TYPE_POINTER,
                    value: VaGenericValueUnion {
                        p: (&mut desc as *mut VaDrmPrimeSurfaceDescriptor).cast(),
                    },
                },
            },
        ];

        let mut surface_id = VA_INVALID_ID;
        // SAFETY: attribs and descriptor point to valid memory on stack with DRM_PRIME_2 layout.
        let status = unsafe {
            (self.va.create_surfaces)(
                self.display,
                VA_RT_FORMAT_RGB32,
                width,
                height,
                &mut surface_id,
                1,
                attribs.as_mut_ptr(),
                2,
            )
        };
        self.va.check(status)?;
        self.imported_surfaces.push(surface_id);
        Ok(surface_id)
    }

    pub fn process_vpp(
        &self,
        src_surface: u32,
        dst_surface: u32,
        source_width: u32,
        source_height: u32,
        y_inverted: bool,
    ) -> Result<(), VaapiError> {
        let region = VaRectangle {
            x: 0,
            y: 0,
            width: self.width as u16,
            height: self.height as u16,
        };

        let (source_region, mirror) = source_transform(source_width, source_height, y_inverted);
        // SAFETY: VaProcPipelineParameterBuffer is zeroed before explicitly filling fields.
        let mut param: VaProcPipelineParameterBuffer = unsafe { std::mem::zeroed() };
        param.surface = src_surface;
        param.surface_region = &source_region;
        param.mirror_state = mirror;
        param.output_region = &region;
        param.filter_flags = 0;
        param.surface_color_standard = VA_PROC_COLOR_STANDARD_EXPLICIT;
        param.output_color_standard = VA_PROC_COLOR_STANDARD_EXPLICIT;

        param.input_color_properties = VaProcColorProperties {
            chroma_sample_location: 0,
            color_range: VA_SOURCE_RANGE_FULL,
            colour_primaries: 1,
            transfer_characteristics: 13,
            matrix_coefficients: 0,
            reserved: [0; 3],
        };

        param.output_color_properties = VaProcColorProperties {
            chroma_sample_location: 0,
            color_range: VA_SOURCE_RANGE_FULL,
            colour_primaries: 1,
            transfer_characteristics: 13,
            matrix_coefficients: 6,
            reserved: [0; 3],
        };

        let mut buf_id = VA_INVALID_ID;
        // SAFETY: param struct matches the 224-byte layout expected by VAProcPipelineParameterBufferType.
        let status = unsafe {
            (self.va.create_buffer)(
                self.display,
                self.context_id,
                VA_PROC_PIPELINE_PARAMETER_BUFFER_TYPE,
                std::mem::size_of::<VaProcPipelineParameterBuffer>() as u32,
                1,
                (&mut param as *mut VaProcPipelineParameterBuffer).cast(),
                &mut buf_id,
            )
        };
        self.va.check(status)?;

        // SAFETY: VPP execution sequence: begin, render with buffer, end, and sync surface.
        let render_res = unsafe {
            let mut res = (self.va.begin_picture)(self.display, self.context_id, dst_surface);
            if res == VA_STATUS_SUCCESS {
                res = (self.va.render_picture)(self.display, self.context_id, &mut buf_id, 1);
            }
            if res == VA_STATUS_SUCCESS {
                res = (self.va.end_picture)(self.display, self.context_id);
            }
            if res == VA_STATUS_SUCCESS {
                res = (self.va.sync_surface)(self.display, dst_surface);
            }
            res
        };

        // SAFETY: buf_id is destroyed after picture rendering completes.
        unsafe {
            (self.va.destroy_buffer)(self.display, buf_id);
        }
        self.va.check(render_res)
    }
}

fn source_transform(width: u32, height: u32, inverted: bool) -> (VaRectangle, u32) {
    (
        VaRectangle {
            x: 0,
            y: 0,
            width: width as u16,
            height: height as u16,
        },
        if inverted { 2 } else { 0 },
    )
}

impl Drop for VaapiPipeline {
    fn drop(&mut self) {
        if !self.imported_surfaces.is_empty() {
            // SAFETY: releasing imported surfaces associated with this pipeline.
            unsafe {
                (self.va.destroy_surfaces)(
                    self.display,
                    self.imported_surfaces.as_mut_ptr(),
                    self.imported_surfaces.len() as u32,
                );
            }
        }
        if self.context_id != VA_INVALID_ID {
            // SAFETY: releasing allocated VPP context.
            unsafe {
                (self.va.destroy_context)(self.display, self.context_id);
            }
        }
        if self.config_id != VA_INVALID_ID {
            // SAFETY: releasing allocated VPP configuration.
            unsafe {
                (self.va.destroy_config)(self.display, self.config_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_geometry_and_vertical_orientation_are_independent_of_output() {
        let (region, mirror) = source_transform(1920, 1080, true);
        assert_eq!((region.width, region.height), (1920, 1080));
        assert_eq!(mirror, 2); // VA_MIRROR_VERTICAL maps the asymmetric top row to bottom.
        let (_, upright) = source_transform(1280, 720, false);
        assert_eq!(upright, 0);
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn vpp_param_buffer_matches_libva_layout() {
        assert_eq!(std::mem::size_of::<VaProcPipelineParameterBuffer>(), 224);
        assert_eq!(
            std::mem::offset_of!(VaProcPipelineParameterBuffer, mirror_state),
            104
        );
        assert_eq!(
            std::mem::offset_of!(VaProcPipelineParameterBuffer, additional_outputs),
            112
        );
        assert_eq!(
            std::mem::offset_of!(VaProcPipelineParameterBuffer, input_color_properties),
            132
        );
        assert_eq!(
            std::mem::offset_of!(VaProcPipelineParameterBuffer, output_color_properties),
            140
        );
        assert_eq!(
            std::mem::offset_of!(VaProcPipelineParameterBuffer, processing_mode),
            148
        );
        assert_eq!(
            std::mem::offset_of!(VaProcPipelineParameterBuffer, output_hdr_metadata),
            152
        );
        assert_eq!(std::mem::size_of::<VaProcColorProperties>(), 8);
    }
}
