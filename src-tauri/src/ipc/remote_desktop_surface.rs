//! Native child-surface presentation for decoded remote desktop NV12 frames.
#![cfg(all(feature = "desktop", feature = "remote-desktop"))]

use ferryx_rd_decode::{ColorMatrix, DecodedVideoFrame, Nv12ColorInfo};

use crate::native_terminal::child_surface::{ChildSurfaceGeometry, GeometryLatch};
use crate::native_terminal::composition::LogicalBounds;
use crate::native_terminal::platform::PlatformCompositorTarget;
use crate::native_terminal::renderer::gpu_context::GpuContext;
use crate::native_terminal::surface_error::{classify_surface_error, SurfaceFrameAction};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RdSurfaceUnavailable {
    #[error("GPU adapter unavailable: {0}")]
    GpuAdapterUnavailable(String),
    #[error("Native composition unsupported: {0}")]
    CompositionUnsupported(String),
    #[error("Frame {width}x{height} exceeds texture dimension limit {limit}")]
    FrameExceedsTextureLimit {
        width: u32,
        height: u32,
        limit: u32,
    },
    #[error("Native surface creation failed: {0}")]
    SurfaceCreationFailed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RdSurfaceError {
    #[error(transparent)]
    Unavailable(#[from] RdSurfaceUnavailable),
    #[error("Native surface presentation failed: {0}")]
    Presentation(String),
}

/// Offset and extent in normalized surface UV coordinates (not frame pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AspectFitRect {
    pub offset_x: f32,
    pub offset_y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
}

pub fn compute_aspect_fit_rect(
    surface_w: u32,
    surface_h: u32,
    frame_w: u32,
    frame_h: u32,
) -> AspectFitRect {
    if surface_w == 0 || surface_h == 0 || frame_w == 0 || frame_h == 0 {
        return AspectFitRect {
            offset_x: 0.0,
            offset_y: 0.0,
            scale_x: 0.0,
            scale_y: 0.0,
        };
    }
    // Match JavaScript's f64 arithmetic, converting only the GPU-facing result to f32.
    let sw = surface_w as f64;
    let sh = surface_h as f64;
    let scale = (sw / frame_w as f64).min(sh / frame_h as f64);
    let drawn_w = frame_w as f64 * scale;
    let drawn_h = frame_h as f64 * scale;
    AspectFitRect {
        offset_x: ((sw - drawn_w) / 2.0 / sw) as f32,
        offset_y: ((sh - drawn_h) / 2.0 / sh) as f32,
        scale_x: (drawn_w / sw) as f32,
        scale_y: (drawn_h / sh) as f32,
    }
}

pub fn uv_to_frame_norm(
    rect: AspectFitRect,
    surface_u: f32,
    surface_v: f32,
) -> Option<(f32, f32)> {
    if rect.scale_x <= 0.0 || rect.scale_y <= 0.0 {
        return None;
    }
    let x = (surface_u - rect.offset_x) / rect.scale_x;
    let y = (surface_v - rect.offset_y) / rect.scale_y;
    if (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) {
        Some((x, y))
    } else {
        None
    }
}

/// Samples are normalized UNORM code values, including the digital chroma center 128/255.
pub fn yuv_to_rgb_cpu(y: f32, u: f32, v: f32, color: Nv12ColorInfo) -> [f32; 3] {
    let mut luma = y;
    let mut cb = u - 128.0 / 255.0;
    let mut cr = v - 128.0 / 255.0;
    if !color.full_range {
        luma = (y - 16.0 / 255.0) * (255.0 / 219.0);
        cb *= 255.0 / 224.0;
        cr *= 255.0 / 224.0;
    }
    let (r_cr, g_cb, g_cr, b_cb) = match color.matrix {
        ColorMatrix::Bt601 => (1.402, 0.344136, 0.714136, 1.772),
        ColorMatrix::Bt709 => (1.5748, 0.187324, 0.468124, 1.8556),
    };
    [
        (luma + r_cr * cr).clamp(0.0, 1.0),
        (luma - g_cb * cb - g_cr * cr).clamp(0.0, 1.0),
        (luma + b_cb * cb).clamp(0.0, 1.0),
    ]
}

pub fn validate_frame_dimensions(
    width: u32,
    height: u32,
    max_dim: u32,
) -> Result<(), RdSurfaceUnavailable> {
    if width > max_dim || height > max_dim {
        return Err(RdSurfaceUnavailable::FrameExceedsTextureLimit {
            width,
            height,
            limit: max_dim,
        });
    }
    if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
        return Err(RdSurfaceUnavailable::SurfaceCreationFailed(format!(
            "NV12 dimensions must be positive and even, got {width}x{height}"
        )));
    }
    Ok(())
}

fn acquire_shared_gpu() -> Result<GpuContext, RdSurfaceUnavailable> {
    GpuContext::shared_surface_context()
        .map_err(|error| RdSurfaceUnavailable::GpuAdapterUnavailable(error.to_string()))
}

fn create_target<R: tauri::Runtime>(
    window: &tauri::Window<R>,
) -> Result<PlatformCompositorTarget, RdSurfaceUnavailable> {
    #[cfg(target_os = "macos")]
    if objc2::MainThreadMarker::new().is_none() {
        let mut result = None;
        // exec_sync borrows this stack frame until the UI operation completes. Never
        // sync-dispatch from the main thread itself (checked above).
        dispatch2::DispatchQueue::main().exec_sync(|| {
            result = Some(PlatformCompositorTarget::new(window));
        });
        return result
            .expect("synchronous main-thread target creation completed")
            .map_err(|error| RdSurfaceUnavailable::SurfaceCreationFailed(error.to_string()));
    }
    PlatformCompositorTarget::new(window)
        .map_err(|error| RdSurfaceUnavailable::SurfaceCreationFailed(error.to_string()))
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniforms {
    rect: [f32; 4],
    matrix_id: u32,
    full_range: u32,
    padding: [u32; 2],
}

struct FrameTextures {
    bind_group: wgpu::BindGroup,
    y_tex: wgpu::Texture,
    uv_tex: wgpu::Texture,
    width: u32,
    height: u32,
}

struct SurfaceGpuResources {
    textures: Option<FrameTextures>,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
}

/// Owns a swapchain, never an instance/device separate from the terminal's shared context.
/// GPU handles precede the native target both here and in the explicit Drop implementation.
pub struct RdNativeSurface {
    surface: Option<wgpu::Surface<'static>>,
    resources: Option<SurfaceGpuResources>,
    target: Option<PlatformCompositorTarget>,
    gpu: GpuContext,
    config: wgpu::SurfaceConfiguration,
    geometry: GeometryLatch<Option<LogicalBounds>>,
    visible: bool,
    revealed: bool,
}

impl Drop for RdNativeSurface {
    fn drop(&mut self) {
        drop(self.surface.take());
        drop(self.resources.take());
        // PlatformCompositorTarget dispatches AppKit teardown to the main thread.
        drop(self.target.take());
    }
}

impl RdNativeSurface {
    pub fn new<R: tauri::Runtime>(window: &tauri::Window<R>) -> Result<Self, RdSurfaceUnavailable> {
        let gpu = acquire_shared_gpu()?;
        let target = create_target(window)?;
        if let Err(error) = target.descriptor().validate_desktop_composition() {
            drop(target);
            return Err(RdSurfaceUnavailable::CompositionUnsupported(
                error.to_string(),
            ));
        }
        #[cfg(target_os = "macos")]
        let surface = unsafe {
            // SAFETY: target retains the CAMetalLayer until after the surface is dropped.
            gpu.instance.create_surface_unsafe(
                wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(
                    target.surface_layer_ptr().expect("macOS target owns a CAMetalLayer"),
                ),
            )
        };
        #[cfg(not(target_os = "macos"))]
        let surface = gpu.instance.create_surface(target.surface_target());
        let surface = surface
            .map_err(|error| RdSurfaceUnavailable::SurfaceCreationFailed(error.to_string()))?;
        let capabilities = surface.get_capabilities(&gpu.adapter);
        // NV12 conversion produces encoded RGB. An sRGB attachment would encode it a
        // second time, so unsupported non-sRGB surfaces use the browser fallback.
        let format = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm]
            .into_iter()
            .find(|format| capabilities.formats.contains(format))
            .ok_or_else(|| {
                RdSurfaceUnavailable::CompositionUnsupported(
                    "Surface has no non-sRGB 8-bit BGRA/RGBA format".into(),
                )
            })?;
        let present_mode = if capabilities.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else if capabilities.present_modes.contains(&wgpu::PresentMode::Fifo) {
            wgpu::PresentMode::Fifo
        } else {
            return Err(RdSurfaceUnavailable::CompositionUnsupported(
                "Surface supports neither Mailbox nor Fifo presentation".into(),
            ));
        };
        let alpha_mode = capabilities
            .alpha_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
            .or_else(|| capabilities.alpha_modes.first().copied())
            .ok_or_else(|| {
                RdSurfaceUnavailable::CompositionUnsupported(
                    "Surface has no composite alpha mode".into(),
                )
            })?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 1,
            height: 1,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&gpu.device, &config);
        let resources = SurfaceGpuResources::new(&gpu.device, format);
        Ok(Self {
            surface: Some(surface),
            resources: Some(resources),
            target: Some(target),
            gpu,
            config,
            geometry: GeometryLatch::default(),
            visible: false,
            revealed: false,
        })
    }

    pub fn update_bounds(&mut self, bounds: Option<LogicalBounds>, visible: bool) {
        let target = self.target.as_ref().expect("live native target");
        // Wayland uses integral logical coordinates and an integer buffer scale.
        let resolved = bounds.and_then(|bounds| match target.presentation_geometry().resolve(bounds) {
            Ok(bounds) => Some(bounds),
            Err(error) => {
                tracing::warn!(%error, "Invalid remote desktop surface bounds");
                None
            }
        });
        let physical = resolved
            .as_ref()
            .and_then(ChildSurfaceGeometry::from_logical_bounds);
        let max_dim = self.gpu.device.limits().max_texture_dimension_2d;
        self.visible = visible
            && physical.is_some_and(|size| size.width <= max_dim && size.height <= max_dim);
        let viewport = if self.visible { resolved } else { None };
        if self.geometry.needs_apply(viewport) {
            target.update_viewport(viewport);
            self.revealed = false;
        }
        if !self.visible {
            return;
        }
        let size = physical.expect("visible bounds have physical dimensions");
        if (self.config.width, self.config.height) != (size.width, size.height) {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface
                .as_ref()
                .expect("live surface")
                .configure(&self.gpu.device, &self.config);
        }
    }

    /// Hidden, timed-out and occluded frames are intentionally dropped without drawing.
    /// Occlusion suspends presentation until a subsequent visible bounds update.
    pub fn present_frame(
        &mut self,
        decoded: &DecodedVideoFrame,
    ) -> Result<(u32, u32), RdSurfaceError> {
        let frame = &decoded.frame;
        let dimensions = (frame.width, frame.height);
        if !self.visible {
            return Ok(dimensions);
        }
        validate_frame_dimensions(
            frame.width,
            frame.height,
            self.gpu.device.limits().max_texture_dimension_2d,
        )?;
        let surface = self.surface.as_ref().expect("live surface");
        let status = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                surface.configure(&self.gpu.device, &self.config);
                surface.get_current_texture()
            }
            status => status,
        };
        let output = match status {
            wgpu::CurrentSurfaceTexture::Success(output)
            | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
            ref other => {
                match classify_surface_error(other)
                    .map_err(|error| RdSurfaceError::Presentation(error.to_string()))?
                {
                    SurfaceFrameAction::Retry => {}
                    SurfaceFrameAction::Suspend => self.visible = false,
                }
                return Ok(dimensions);
            }
        };
        let resources = self.resources.as_mut().expect("live surface GPU resources");
        if resources.textures.as_ref().map(|textures| (textures.width, textures.height))
            != Some(dimensions)
        {
            resources.textures = Some(resources.create_frame_textures(
                &self.gpu.device,
                frame.width,
                frame.height,
            ));
        }
        let textures = resources.textures.as_ref().expect("frame textures initialized");
        for (texture, plane, stride, width, height) in [
            (
                &textures.y_tex,
                &frame.y_plane,
                frame.y_stride,
                frame.width,
                frame.height,
            ),
            (
                &textures.uv_tex,
                &frame.uv_plane,
                frame.uv_stride,
                frame.width / 2,
                frame.height / 2,
            ),
        ] {
            self.gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                plane,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride as u32),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
        let rect = compute_aspect_fit_rect(
            self.config.width,
            self.config.height,
            frame.width,
            frame.height,
        );
        let uniforms = FrameUniforms {
            rect: [rect.offset_x, rect.offset_y, rect.scale_x, rect.scale_y],
            matrix_id: match decoded.color.matrix {
                ColorMatrix::Bt601 => 0,
                ColorMatrix::Bt709 => 1,
            },
            full_range: u32::from(decoded.color.full_range),
            padding: [0; 2],
        };
        self.gpu
            .queue
            .write_buffer(&resources.uniform, 0, bytemuck::bytes_of(&uniforms));
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Remote Desktop NV12 Encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Remote Desktop NV12 Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&resources.pipeline);
            pass.set_bind_group(0, &textures.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.gpu.queue.submit(Some(encoder.finish()));
        output.present();
        if !self.revealed {
            self.target
                .as_ref()
                .expect("live native target")
                .reveal_after_present();
            self.revealed = true;
        }
        Ok(dimensions)
    }
}

impl SurfaceGpuResources {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let texture_binding = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Remote Desktop NV12 Layout"),
            entries: &[
                texture_binding(0),
                texture_binding(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<FrameUniforms>() as u64,
                        ),
                    },
                    count: None,
                },
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Remote Desktop Frame Uniforms"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Remote Desktop NV12 Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Remote Desktop NV12 Shader"),
            source: wgpu::ShaderSource::Wgsl(NV12_SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Remote Desktop NV12 Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Remote Desktop NV12 Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            textures: None,
            pipeline,
            bind_group_layout,
            uniform,
            sampler,
        }
    }

    fn create_frame_textures(&self, device: &wgpu::Device, width: u32, height: u32) -> FrameTextures {
        let create_texture = |label, width, height, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let y_tex = create_texture(
            "Remote Desktop NV12 Y",
            width,
            height,
            wgpu::TextureFormat::R8Unorm,
        );
        let uv_tex = create_texture(
            "Remote Desktop NV12 UV",
            width / 2,
            height / 2,
            wgpu::TextureFormat::Rg8Unorm,
        );
        let y_view = y_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let uv_view = uv_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Remote Desktop NV12 Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&y_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&uv_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        });
        FrameTextures {
            bind_group,
            y_tex,
            uv_tex,
            width,
            height,
        }
    }
}

const NV12_SHADER: &str = r#"
struct FrameUniforms {
    rect: vec4<f32>,
    matrix_id: u32,
    full_range: u32,
    padding: vec2<u32>,
}
@group(0) @binding(0) var y_tex: texture_2d<f32>;
@group(0) @binding(1) var uv_tex: texture_2d<f32>;
@group(0) @binding(2) var frame_sampler: sampler;
@group(0) @binding(3) var<uniform> frame: FrameUniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var output: VertexOutput;
    output.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    output.uv = uv;
    return output;
}

fn yuv_to_rgb(y: f32, u: f32, v: f32) -> vec3<f32> {
    var luma = y;
    var cb = u - 128.0 / 255.0;
    var cr = v - 128.0 / 255.0;
    if frame.full_range == 0u {
        luma = (y - 16.0 / 255.0) * (255.0 / 219.0);
        cb *= 255.0 / 224.0;
        cr *= 255.0 / 224.0;
    }
    var coefficients = vec4<f32>(1.402, 0.344136, 0.714136, 1.772);
    if frame.matrix_id == 1u {
        coefficients = vec4<f32>(1.5748, 0.187324, 0.468124, 1.8556);
    }
    return clamp(vec3<f32>(
        luma + coefficients.x * cr,
        luma - coefficients.y * cb - coefficients.z * cr,
        luma + coefficients.w * cb,
    ), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = (input.uv - frame.rect.xy) / frame.rect.zw;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    // Explicit LOD keeps sampling valid after the non-uniform letterbox branch.
    let y = textureSampleLevel(y_tex, frame_sampler, uv, 0.0).r;
    let chroma = textureSampleLevel(uv_tex, frame_sampler, uv, 0.0).rg;
    return vec4<f32>(yuv_to_rgb(y, chroma.r, chroma.g), 1.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_fit_matches_client_point_mapping_with_subpixel_precision() {
        for (sw, sh, fw, fh) in [
            (800, 600, 1920, 1080),
            (1920, 1080, 800, 600),
            (1024, 1024, 1024, 1024),
            (1710, 1112, 1920, 1080),
            (1710, 1112, 2560, 1440),
            (1920, 1080, 1710, 1112),
            (2560, 1440, 1710, 1112),
        ] {
            let rect = compute_aspect_fit_rect(sw, sh, fw, fh);
            // Independent f64 reference: the exact mapClientPointToFrame arithmetic.
            let scale = (sw as f64 / fw as f64).min(sh as f64 / fh as f64);
            let drawn_w = fw as f64 * scale;
            let drawn_h = fh as f64 * scale;
            let offset_x = (sw as f64 - drawn_w) / 2.0;
            let offset_y = (sh as f64 - drawn_h) / 2.0;
            for (x, y) in [(0.001, 0.001), (0.25, 0.75), (0.5, 0.5), (0.999, 0.999)] {
                let client_x = offset_x + drawn_w * x;
                let client_y = offset_y + drawn_h * y;
                let (actual_x, actual_y) = uv_to_frame_norm(
                    rect,
                    (client_x / sw as f64) as f32,
                    (client_y / sh as f64) as f32,
                )
                .expect("point inside the drawn frame");
                let expected_x = (client_x - offset_x) / drawn_w;
                let expected_y = (client_y - offset_y) / drawn_h;
                assert!((actual_x as f64 - expected_x).abs() * (fw as f64) < 0.5);
                assert!((actual_y as f64 - expected_y).abs() * (fh as f64) < 0.5);
            }
            if offset_x > 0.0 {
                assert_eq!(uv_to_frame_norm(rect, 0.0, 0.5), None);
            }
            if offset_y > 0.0 {
                assert_eq!(uv_to_frame_norm(rect, 0.5, 0.0), None);
            }
        }
    }

    #[test]
    fn aspect_fit_edges_are_inclusive_and_empty_rectangles_do_not_map() {
        let rect = compute_aspect_fit_rect(100, 100, 100, 100);
        assert_eq!(uv_to_frame_norm(rect, 0.0, 0.0), Some((0.0, 0.0)));
        assert_eq!(uv_to_frame_norm(rect, 1.0, 1.0), Some((1.0, 1.0)));
        assert_eq!(uv_to_frame_norm(rect, -0.01, 0.5), None);
        assert_eq!(uv_to_frame_norm(rect, 0.5, 1.01), None);
        assert_eq!(uv_to_frame_norm(rect, f32::NAN, 0.5), None);
        for dims in [(0, 100, 100, 100), (100, 0, 100, 100), (100, 100, 0, 100)] {
            assert_eq!(
                uv_to_frame_norm(
                    compute_aspect_fit_rect(dims.0, dims.1, dims.2, dims.3),
                    0.5,
                    0.5,
                ),
                None,
            );
        }
    }

    #[test]
    fn both_color_matrices_preserve_black_white_and_neutral_gray_in_both_ranges() {
        for matrix in [ColorMatrix::Bt601, ColorMatrix::Bt709] {
            for full_range in [false, true] {
                let color = Nv12ColorInfo {
                    matrix,
                    full_range,
                };
                let (black, white, gray, expected_gray) = if full_range {
                    (0.0, 1.0, 128.0 / 255.0, 128.0 / 255.0)
                } else {
                    (16.0 / 255.0, 235.0 / 255.0, 126.0 / 255.0, 110.0 / 219.0)
                };
                for (y, expected) in [(black, 0.0), (white, 1.0), (gray, expected_gray)] {
                    for channel in yuv_to_rgb_cpu(y, 128.0 / 255.0, 128.0 / 255.0, color) {
                        assert!((channel - expected).abs() < 1e-6);
                    }
                }
            }
        }
    }

    #[test]
    fn chromatic_samples_distinguish_bt601_from_bt709() {
        for (matrix, expected) in [
            (ColorMatrix::Bt601, [0.6402, 0.46299997, 0.3228]),
            (ColorMatrix::Bt709, [0.65748, 0.47192, 0.31444]),
        ] {
            let rgb = yuv_to_rgb_cpu(
                0.5,
                128.0 / 255.0 - 0.1,
                128.0 / 255.0 + 0.1,
                Nv12ColorInfo {
                    matrix,
                    full_range: true,
                },
            );
            for (actual, expected) in rgb.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn invalid_nv12_dimensions_are_rejected_before_allocating_textures() {
        for (width, height) in [(0, 1080), (1920, 0), (1919, 1080), (1920, 1079)] {
            assert!(validate_frame_dimensions(width, height, 8192).is_err());
        }
        assert_eq!(
            validate_frame_dimensions(16384, 1080, 8192),
            Err(RdSurfaceUnavailable::FrameExceedsTextureLimit {
                width: 16384,
                height: 1080,
                limit: 8192,
            })
        );
        assert_eq!(validate_frame_dimensions(8192, 8192, 8192), Ok(()));
        assert_eq!(validate_frame_dimensions(1920, 1080, 8192), Ok(()));
    }

    #[test]
    fn forced_adapter_failure_is_typed_without_initializing_a_gpu_or_display() {
        // The thread-local ForcedAdapterFailure seam exercises the same path as
        // FERRYX_FORCE_NO_GPU_ADAPTER without racing process-wide environment changes.
        let _forced = GpuContext::force_adapter_unavailable_for_test();
        assert!(matches!(
            acquire_shared_gpu(),
            Err(RdSurfaceUnavailable::GpuAdapterUnavailable(_))
        ));
    }
}
