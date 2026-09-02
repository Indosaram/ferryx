//! WGPU rendering pipelines for terminal background and glyph passes.

use bytemuck::{Pod, Zeroable};

use super::shaders::{BG_SHADER_SRC, GLYPH_SHADER_SRC};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct ScreenUniform {
    pub screen_size: [f32; 2],
    pub _pad: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct RectInstance {
    pub rect: [f32; 4],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct GlyphInstance {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub is_color: f32,
    pub _pad: [f32; 3],
}

pub struct RenderPipelines {
    pub rgba_bg: wgpu::RenderPipeline,
    pub rgba_glyph: wgpu::RenderPipeline,
    pub rgba_overlay: wgpu::RenderPipeline,
    pub bgra_bg: wgpu::RenderPipeline,
    pub bgra_glyph: wgpu::RenderPipeline,
    pub bgra_overlay: wgpu::RenderPipeline,
    pub rgba_srgb_bg: wgpu::RenderPipeline,
    pub rgba_srgb_glyph: wgpu::RenderPipeline,
    pub rgba_srgb_overlay: wgpu::RenderPipeline,
    pub bgra_srgb_bg: wgpu::RenderPipeline,
    pub bgra_srgb_glyph: wgpu::RenderPipeline,
    pub bgra_srgb_overlay: wgpu::RenderPipeline,
    pub uniform_layout: wgpu::BindGroupLayout,
    pub atlas_layout: wgpu::BindGroupLayout,
}

impl RenderPipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Screen Uniform Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Atlas Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Background Shader"),
            source: wgpu::ShaderSource::Wgsl(BG_SHADER_SRC.into()),
        });
        let bg_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Background Pipeline Layout"),
            bind_group_layouts: &[&uniform_layout],
            push_constant_ranges: &[],
        });
        let glyph_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Glyph Shader"),
            source: wgpu::ShaderSource::Wgsl(GLYPH_SHADER_SRC.into()),
        });
        let glyph_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Glyph Pipeline Layout"),
            bind_group_layouts: &[&uniform_layout, &atlas_layout],
            push_constant_ranges: &[],
        });

        Self {
            rgba_bg: build_bg_pipeline(
                device,
                &bg_shader,
                &bg_layout,
                wgpu::TextureFormat::Rgba8Unorm,
            ),
            rgba_glyph: build_glyph_pipeline(
                device,
                &glyph_shader,
                &glyph_layout,
                wgpu::TextureFormat::Rgba8Unorm,
            ),
            rgba_overlay: build_overlay_pipeline(
                device,
                &bg_shader,
                &bg_layout,
                wgpu::TextureFormat::Rgba8Unorm,
            ),
            bgra_bg: build_bg_pipeline(
                device,
                &bg_shader,
                &bg_layout,
                wgpu::TextureFormat::Bgra8Unorm,
            ),
            bgra_glyph: build_glyph_pipeline(
                device,
                &glyph_shader,
                &glyph_layout,
                wgpu::TextureFormat::Bgra8Unorm,
            ),
            bgra_overlay: build_overlay_pipeline(
                device,
                &bg_shader,
                &bg_layout,
                wgpu::TextureFormat::Bgra8Unorm,
            ),
            rgba_srgb_bg: build_bg_pipeline(
                device,
                &bg_shader,
                &bg_layout,
                wgpu::TextureFormat::Rgba8UnormSrgb,
            ),
            rgba_srgb_glyph: build_glyph_pipeline(
                device,
                &glyph_shader,
                &glyph_layout,
                wgpu::TextureFormat::Rgba8UnormSrgb,
            ),
            rgba_srgb_overlay: build_overlay_pipeline(
                device,
                &bg_shader,
                &bg_layout,
                wgpu::TextureFormat::Rgba8UnormSrgb,
            ),
            bgra_srgb_bg: build_bg_pipeline(
                device,
                &bg_shader,
                &bg_layout,
                wgpu::TextureFormat::Bgra8UnormSrgb,
            ),
            bgra_srgb_glyph: build_glyph_pipeline(
                device,
                &glyph_shader,
                &glyph_layout,
                wgpu::TextureFormat::Bgra8UnormSrgb,
            ),
            bgra_srgb_overlay: build_overlay_pipeline(
                device,
                &bg_shader,
                &bg_layout,
                wgpu::TextureFormat::Bgra8UnormSrgb,
            ),
            uniform_layout,
            atlas_layout,
        }
    }

    pub fn get_pipelines(
        &self,
        format: wgpu::TextureFormat,
    ) -> (&wgpu::RenderPipeline, &wgpu::RenderPipeline) {
        match format {
            wgpu::TextureFormat::Bgra8Unorm => (&self.bgra_bg, &self.bgra_glyph),
            wgpu::TextureFormat::Bgra8UnormSrgb => (&self.bgra_srgb_bg, &self.bgra_srgb_glyph),
            wgpu::TextureFormat::Rgba8UnormSrgb => (&self.rgba_srgb_bg, &self.rgba_srgb_glyph),
            _ => (&self.rgba_bg, &self.rgba_glyph),
        }
    }

    pub fn get_overlay_pipeline(&self, format: wgpu::TextureFormat) -> &wgpu::RenderPipeline {
        match format {
            wgpu::TextureFormat::Bgra8Unorm => &self.bgra_overlay,
            wgpu::TextureFormat::Bgra8UnormSrgb => &self.bgra_srgb_overlay,
            wgpu::TextureFormat::Rgba8UnormSrgb => &self.rgba_srgb_overlay,
            _ => &self.rgba_overlay,
        }
    }
}

fn build_overlay_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Overlay Render Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<RectInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn build_bg_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Background Render Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<RectInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn build_glyph_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Glyph Render Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<GlyphInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}
