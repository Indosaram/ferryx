//! WGPU pass encoding helpers for terminal background rects and glyph quads.

use super::images::ImageDraw;
use super::pipeline::{GlyphInstance, RectInstance, RenderPipelines, ScreenUniform};
use crate::native_terminal::composition::PhysicalBounds;

#[derive(Default)]
struct GrowableVertexBuffer {
    buffer: Option<wgpu::Buffer>,
    capacity_bytes: u64,
    #[cfg(test)]
    allocations: usize,
}

impl GrowableVertexBuffer {
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &'static str,
        bytes: &[u8],
    ) -> Option<u64> {
        if bytes.is_empty() {
            return None;
        }
        let needed = bytes.len() as u64;
        if self.buffer.is_none() || self.capacity_bytes < needed {
            let capacity = needed
                .checked_next_power_of_two()
                .unwrap_or(needed)
                .max(256);
            let aligned_capacity = (capacity + 3) & !3;
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: aligned_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity_bytes = aligned_capacity;
            #[cfg(test)]
            {
                self.allocations += 1;
            }
        }
        let buf = self.buffer.as_ref().expect("vertex buffer allocated");
        queue.write_buffer(buf, 0, bytes);
        Some(needed)
    }

    fn slice(&self, len: u64) -> wgpu::BufferSlice<'_> {
        self.buffer
            .as_ref()
            .expect("vertex buffer uploaded before slicing")
            .slice(0..len)
    }
}

/// Reusable grow-only vertex buffers for terminal render passes, avoiding per-frame
/// GPU buffer allocations and deallocations.
#[derive(Default)]
pub struct FrameVertexBuffers {
    bg: GrowableVertexBuffer,
    glyph: GrowableVertexBuffer,
    overlay: GrowableVertexBuffer,
    occluder: GrowableVertexBuffer,
}

#[cfg(test)]
impl FrameVertexBuffers {
    pub(crate) fn allocation_counts(&self) -> (usize, usize, usize, usize) {
        (
            self.bg.allocations,
            self.glyph.allocations,
            self.overlay.allocations,
            self.occluder.allocations,
        )
    }
}

pub struct TerminalPassResources<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub pipelines: &'a RenderPipelines,
    pub uniform_buf: &'a wgpu::Buffer,
    pub uniform_bg: &'a wgpu::BindGroup,
    pub atlas_bg: &'a wgpu::BindGroup,
}

pub struct TerminalPassTarget<'a> {
    pub view: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    pub width: u32,
    pub height: u32,
}

pub struct TerminalPassOptions<'a> {
    pub clear_color: wgpu::Color,
    pub scissor: Option<PhysicalBounds>,
    pub overlay: &'a [RectInstance],
    pub images: &'a [ImageDraw],
    pub image_occluders: &'a [RectInstance],
}

pub fn encode_terminal_passes(
    resources: TerminalPassResources<'_>,
    encoder: &mut wgpu::CommandEncoder,
    target: TerminalPassTarget<'_>,
    bg: &[RectInstance],
    glyph: &[GlyphInstance],
) {
    let mut scratch = FrameVertexBuffers::default();
    encode_terminal_passes_with_surface_options(
        resources,
        encoder,
        target,
        bg,
        glyph,
        &mut scratch,
        TerminalPassOptions {
            clear_color: wgpu::Color {
                r: 0.07,
                g: 0.07,
                b: 0.09,
                a: 1.0,
            },
            scissor: None,
            overlay: &[],
            images: &[],
            image_occluders: &[],
        },
    );
}

pub fn encode_terminal_passes_with_surface_options(
    resources: TerminalPassResources<'_>,
    encoder: &mut wgpu::CommandEncoder,
    target: TerminalPassTarget<'_>,
    bg: &[RectInstance],
    glyph: &[GlyphInstance],
    vertex_buffers: &mut FrameVertexBuffers,
    options: TerminalPassOptions<'_>,
) {
    let TerminalPassResources { device, queue, pipelines, uniform_buf, uniform_bg, atlas_bg } = resources;
    let TerminalPassTarget { view, format, width: w, height: h } = target;
    let TerminalPassOptions { clear_color, scissor, overlay, images, image_occluders } = options;
    queue.write_buffer(
        uniform_buf,
        0,
        bytemuck::bytes_of(&ScreenUniform {
            screen_size: [w as f32, h as f32],
            _pad: [0.0, 0.0],
        }),
    );
    let bg_bytes = vertex_buffers.bg.upload(
        device,
        queue,
        "Background Buffer",
        bytemuck::cast_slice(bg),
    );
    let glyph_bytes = vertex_buffers.glyph.upload(
        device,
        queue,
        "Glyph Buffer",
        bytemuck::cast_slice(glyph),
    );
    let overlay_bytes = vertex_buffers.overlay.upload(
        device,
        queue,
        "Overlay Buffer",
        bytemuck::cast_slice(overlay),
    );
    let occluder_bytes = vertex_buffers.occluder.upload(
        device,
        queue,
        "Kitty background occluders",
        bytemuck::cast_slice(image_occluders),
    );
    let (bg_pipe, glyph_pipe) = pipelines.get_pipelines(format);
    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Terminal Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear_color),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(bounds) = scissor {
        rpass.set_scissor_rect(bounds.x, bounds.y, bounds.width, bounds.height);
    }
    if let Some(bytes) = bg_bytes {
        rpass.set_pipeline(bg_pipe);
        rpass.set_bind_group(0, uniform_bg, &[]);
        rpass.set_vertex_buffer(0, vertex_buffers.bg.slice(bytes));
        rpass.draw(0..6, 0..bg.len() as u32);
    }
    draw_images(&mut rpass, images, glyph_pipe, uniform_bg, |z| {
        z < i32::MIN / 2
    });
    if let Some(bytes) = occluder_bytes {
        rpass.set_pipeline(bg_pipe);
        rpass.set_bind_group(0, uniform_bg, &[]);
        rpass.set_vertex_buffer(0, vertex_buffers.occluder.slice(bytes));
        rpass.draw(0..6, 0..image_occluders.len() as u32);
    }
    draw_images(&mut rpass, images, glyph_pipe, uniform_bg, |z| {
        (i32::MIN / 2..0).contains(&z)
    });
    if let Some(bytes) = glyph_bytes {
        rpass.set_pipeline(glyph_pipe);
        rpass.set_bind_group(0, uniform_bg, &[]);
        rpass.set_bind_group(1, atlas_bg, &[]);
        rpass.set_vertex_buffer(0, vertex_buffers.glyph.slice(bytes));
        rpass.draw(0..6, 0..glyph.len() as u32);
    }
    draw_images(&mut rpass, images, glyph_pipe, uniform_bg, |z| z >= 0);
    if let Some(bytes) = overlay_bytes {
        // Surface overlays remain above every Kitty z layer.
        let overlay_pipe = pipelines.get_overlay_pipeline(format);
        rpass.set_pipeline(overlay_pipe);
        rpass.set_bind_group(0, uniform_bg, &[]);
        rpass.set_vertex_buffer(0, vertex_buffers.overlay.slice(bytes));
        rpass.draw(0..6, 0..overlay.len() as u32);
    }
}

fn draw_images<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    images: &'a [ImageDraw],
    pipeline: &'a wgpu::RenderPipeline,
    uniform: &'a wgpu::BindGroup,
    in_layer: impl Fn(i32) -> bool,
) {
    for image in images.iter().filter(|i| in_layer(i.z)) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, uniform, &[]);
        pass.set_bind_group(1, &image.binding, &[]);
        pass.set_vertex_buffer(0, image.vertices.slice(..));
        pass.draw(0..6, 0..1);
    }
}
