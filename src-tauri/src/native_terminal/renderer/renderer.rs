//! Ferryx Native Terminal WGPU Renderer Implementation.

use wgpu::util::DeviceExt;

use super::atlas::GlyphAtlas;
use super::gpu_context::GpuContext;
use super::pass::encode_terminal_passes_with_surface_options;
use super::pipeline::{RectInstance, RenderPipelines, ScreenUniform};
use super::render_target::{RenderTarget, TARGET_FORMAT};
use super::row_cache::RowCacheManager;
use super::types::{GlyphAtlasStats, OffscreenFrame, RendererConfig, SelectionSnapshot};
use crate::native_terminal::composition::PhysicalBounds;
use crate::native_terminal::error::NativeTerminalError;
use crate::native_terminal::snapshot::RenderSnapshot;

pub struct NativeTerminalRenderer {
    config: RendererConfig,
    gpu: GpuContext,
    pipelines: RenderPipelines,
    atlas: GlyphAtlas,
    target: Option<RenderTarget>,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    atlas_bind_group: wgpu::BindGroup,
    row_cache: RowCacheManager,
}

impl NativeTerminalRenderer {
    pub fn new(config: RendererConfig) -> Result<Self, NativeTerminalError> {
        config.validate()?;

        let gpu = GpuContext::new()?;
        let pipelines = RenderPipelines::new(&gpu.device);
        let atlas = GlyphAtlas::new(&gpu.device);

        let uniform_buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Screen Uniform Buffer"),
                contents: bytemuck::bytes_of(&ScreenUniform {
                    screen_size: [1.0, 1.0],
                    _pad: [0.0, 0.0],
                }),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let uniform_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Uniform Bind Group"),
            layout: &pipelines.uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let atlas_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Atlas Bind Group"),
            layout: &pipelines.atlas_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas.mask_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas.color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas.sampler),
                },
            ],
        });

        Ok(Self {
            config,
            gpu,
            pipelines,
            atlas,
            target: None,
            uniform_buffer,
            uniform_bind_group,
            atlas_bind_group,
            row_cache: RowCacheManager::new(),
        })
    }

    /// Returns a reference to the active renderer configuration.
    pub fn config(&self) -> &RendererConfig {
        &self.config
    }

    /// Safely updates renderer configuration and invalidates atlas/cache on dimension/scale change.
    pub fn update_config(&mut self, config: RendererConfig) -> Result<(), NativeTerminalError> {
        config.validate()?;
        if self.config != config {
            self.config = config;
            self.atlas.clear();
            self.target = None;
        }
        Ok(())
    }

    /// Provides access to the underlying WGPU adapter metadata.
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.gpu.adapter_info
    }

    /// Creates a native render surface using the renderer's WGPU instance.
    pub fn create_surface<'a>(
        &self,
        target: impl Into<wgpu::SurfaceTarget<'a>>,
    ) -> Result<wgpu::Surface<'a>, NativeTerminalError> {
        self.gpu.create_surface(target)
    }

    /// Configures a native surface with matching device capabilities and returns its format.
    pub fn configure_surface(
        &self,
        surface: &wgpu::Surface,
        width: u32,
        height: u32,
    ) -> Result<wgpu::TextureFormat, NativeTerminalError> {
        self.gpu.configure_surface(surface, width, height)
    }

    pub fn render_snapshot(
        &mut self,
        snapshot: &RenderSnapshot,
        selection: Option<&SelectionSnapshot>,
    ) -> Result<OffscreenFrame, NativeTerminalError> {
        let (width_px, height_px) = self.validate_and_dims(snapshot)?;
        let (bg, glyph, rebuilt, reused) = self.row_cache.update_and_flatten(
            snapshot,
            selection,
            &self.config,
            &mut self.atlas,
            &self.gpu.queue,
        );

        if self
            .target
            .as_ref()
            .map_or(true, |t| t.width != width_px || t.height != height_px)
        {
            self.target = Some(RenderTarget::new(&self.gpu.device, width_px, height_px));
        }
        let target = self
            .target
            .as_ref()
            .ok_or_else(|| NativeTerminalError::GpuBufferError("Target missing".into()))?;

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Offscreen Encoder"),
            });

        let clear_color = wgpu::Color {
            r: self.config.theme.background[0] as f64,
            g: self.config.theme.background[1] as f64,
            b: self.config.theme.background[2] as f64,
            a: self.config.theme.background[3] as f64,
        };

        encode_terminal_passes_with_surface_options(
            &self.gpu.device,
            &self.gpu.queue,
            &self.pipelines,
            &self.uniform_buffer,
            &self.uniform_bind_group,
            &self.atlas_bind_group,
            &mut encoder,
            &target.view,
            TARGET_FORMAT,
            width_px,
            height_px,
            &bg,
            &glyph,
            clear_color,
            None,
        );
        target.copy_to_staging(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));

        self.gpu.check_error()?;
        let mut frame = target.readback_frame(&self.gpu.device, snapshot.rows)?;
        self.gpu.check_error()?;

        frame.rebuilt_row_count = rebuilt;
        frame.reused_row_count = reused;
        Ok(frame)
    }

    /// Renders snapshot passes directly into an external GPU TextureView (e.g. swapchain surface).
    pub fn render_to_surface_view(
        &mut self,
        snapshot: &RenderSnapshot,
        selection: Option<&SelectionSnapshot>,
        view: &wgpu::TextureView,
        width_px: u32,
        height_px: u32,
        format: wgpu::TextureFormat,
    ) -> Result<(u16, u16), NativeTerminalError> {
        let (_, _) = self.validate_and_dims(snapshot)?;
        let (bg, glyph, rebuilt, reused) = self.row_cache.update_and_flatten(
            snapshot,
            selection,
            &self.config,
            &mut self.atlas,
            &self.gpu.queue,
        );

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Surface Encoder"),
            });

        let clear_color = wgpu::Color {
            r: self.config.theme.background[0] as f64,
            g: self.config.theme.background[1] as f64,
            b: self.config.theme.background[2] as f64,
            a: self.config.theme.background[3] as f64,
        };

        encode_terminal_passes_with_surface_options(
            &self.gpu.device,
            &self.gpu.queue,
            &self.pipelines,
            &self.uniform_buffer,
            &self.uniform_bind_group,
            &self.atlas_bind_group,
            &mut encoder,
            view,
            format,
            width_px,
            height_px,
            &bg,
            &glyph,
            clear_color,
            None,
        );
        self.gpu.queue.submit(Some(encoder.finish()));
        self.gpu.check_error()?;

        Ok((rebuilt, reused))
    }

    pub fn render_to_surface_viewport(
        &mut self,
        snapshot: &RenderSnapshot,
        selection: Option<&SelectionSnapshot>,
        view: &wgpu::TextureView,
        surface_width_px: u32,
        surface_height_px: u32,
        format: wgpu::TextureFormat,
        viewport: PhysicalBounds,
    ) -> Result<(u16, u16), NativeTerminalError> {
        let (terminal_width_px, terminal_height_px) = self.validate_and_dims(snapshot)?;
        let right = viewport
            .x
            .checked_add(viewport.width)
            .ok_or(NativeTerminalError::LimitExceeded)?;
        let bottom = viewport
            .y
            .checked_add(viewport.height)
            .ok_or(NativeTerminalError::LimitExceeded)?;
        if viewport.width < terminal_width_px
            || viewport.height < terminal_height_px
            || right > surface_width_px
            || bottom > surface_height_px
        {
            return Err(NativeTerminalError::InvalidValue(
                "Native terminal viewport is outside the configured surface".into(),
            ));
        }

        let (bg, mut glyph, rebuilt, reused) = self.row_cache.update_and_flatten(
            snapshot,
            selection,
            &self.config,
            &mut self.atlas,
            &self.gpu.queue,
        );

        let default_bg_color = self.config.theme.background;
        let mut final_bg = Vec::with_capacity(bg.len() + 1);
        // Base viewport rectangle ensuring 100% native child view coverage including any residual gutter
        final_bg.push(RectInstance {
            rect: [
                viewport.x as f32,
                viewport.y as f32,
                viewport.width as f32,
                viewport.height as f32,
            ],
            color: default_bg_color,
        });

        for mut instance in bg {
            instance.rect[0] += viewport.x as f32;
            instance.rect[1] += viewport.y as f32;
            final_bg.push(instance);
        }
        for instance in &mut glyph {
            instance.rect[0] += viewport.x as f32;
            instance.rect[1] += viewport.y as f32;
        }

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Native Terminal Surface Encoder"),
            });
        encode_terminal_passes_with_surface_options(
            &self.gpu.device,
            &self.gpu.queue,
            &self.pipelines,
            &self.uniform_buffer,
            &self.uniform_bind_group,
            &self.atlas_bind_group,
            &mut encoder,
            view,
            format,
            surface_width_px,
            surface_height_px,
            &final_bg,
            &glyph,
            wgpu::Color {
                r: default_bg_color[0] as f64,
                g: default_bg_color[1] as f64,
                b: default_bg_color[2] as f64,
                a: default_bg_color[3] as f64,
            },
            Some(viewport),
        );
        self.gpu.queue.submit(Some(encoder.finish()));
        self.gpu.check_error()?;
        Ok((rebuilt, reused))
    }

    /// Renders snapshot passes to an offscreen viewport target with readback (for verification and testing).
    pub fn render_to_offscreen_viewport(
        &mut self,
        snapshot: &RenderSnapshot,
        selection: Option<&SelectionSnapshot>,
        surface_width_px: u32,
        surface_height_px: u32,
        viewport: PhysicalBounds,
    ) -> Result<OffscreenFrame, NativeTerminalError> {
        let target = RenderTarget::new(&self.gpu.device, surface_width_px, surface_height_px);
        let (rebuilt, reused) = self.render_to_surface_viewport(
            snapshot,
            selection,
            &target.view,
            surface_width_px,
            surface_height_px,
            TARGET_FORMAT,
            viewport,
        )?;

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Offscreen Viewport Readback Encoder"),
            });
        target.copy_to_staging(&mut encoder);
        self.gpu.queue.submit(Some(encoder.finish()));
        self.gpu.check_error()?;

        let mut frame = target.readback_frame(&self.gpu.device, snapshot.rows)?;
        self.gpu.check_error()?;
        frame.rebuilt_row_count = rebuilt;
        frame.reused_row_count = reused;
        Ok(frame)
    }

    pub fn glyph_atlas_stats(&self) -> GlyphAtlasStats {
        self.atlas.stats()
    }

    fn validate_and_dims(&self, snap: &RenderSnapshot) -> Result<(u32, u32), NativeTerminalError> {
        if snap.cols == 0 || snap.rows == 0 {
            return Err(NativeTerminalError::InvalidDimensions(snap.cols, snap.rows));
        }
        let w = (snap.cols as u32)
            .checked_mul(self.config.cell_width_px)
            .ok_or_else(|| NativeTerminalError::InvalidDimensions(snap.cols, snap.rows))?;
        let h = (snap.rows as u32)
            .checked_mul(self.config.cell_height_px)
            .ok_or_else(|| NativeTerminalError::InvalidDimensions(snap.cols, snap.rows))?;
        if w == 0 || h == 0 || w > 16384 || h > 16384 {
            return Err(NativeTerminalError::InvalidDimensions(snap.cols, snap.rows));
        }
        Ok((w, h))
    }
}
