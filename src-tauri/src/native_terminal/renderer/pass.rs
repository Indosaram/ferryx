//! WGPU pass encoding helpers for terminal background rects and glyph quads.

use wgpu::util::DeviceExt;

use super::pipeline::{GlyphInstance, RectInstance, RenderPipelines, ScreenUniform};
use super::images::ImageDraw;
use crate::native_terminal::composition::PhysicalBounds;

pub fn encode_terminal_passes(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &RenderPipelines,
    uniform_buf: &wgpu::Buffer,
    uniform_bg: &wgpu::BindGroup,
    atlas_bg: &wgpu::BindGroup,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    format: wgpu::TextureFormat,
    w: u32,
    h: u32,
    bg: &[RectInstance],
    glyph: &[GlyphInstance],
) {
    encode_terminal_passes_with_surface_options(
        device,
        queue,
        pipelines,
        uniform_buf,
        uniform_bg,
        atlas_bg,
        encoder,
        view,
        format,
        w,
        h,
        bg,
        glyph,
        wgpu::Color {
            r: 0.07,
            g: 0.07,
            b: 0.09,
            a: 1.0,
        },
        None,
        &[],
        &[],
        &[],
    );
}

pub fn encode_terminal_passes_with_surface_options(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &RenderPipelines,
    uniform_buf: &wgpu::Buffer,
    uniform_bg: &wgpu::BindGroup,
    atlas_bg: &wgpu::BindGroup,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    format: wgpu::TextureFormat,
    w: u32,
    h: u32,
    bg: &[RectInstance],
    glyph: &[GlyphInstance],
    clear_color: wgpu::Color,
    scissor: Option<PhysicalBounds>,
    overlay: &[RectInstance],
    images: &[ImageDraw],
    image_occluders: &[RectInstance],
) {
    queue.write_buffer(
        uniform_buf,
        0,
        bytemuck::bytes_of(&ScreenUniform {
            screen_size: [w as f32, h as f32],
            _pad: [0.0, 0.0],
        }),
    );
    let bg_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Background Buffer"),
        contents: bytemuck::cast_slice(bg),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let glyph_buf = if !glyph.is_empty() {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Glyph Buffer"),
                contents: bytemuck::cast_slice(glyph),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
    } else {
        None
    };
    let overlay_buf = if !overlay.is_empty() {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Overlay Buffer"),
                contents: bytemuck::cast_slice(overlay),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
    } else {
        None
    };
    let (bg_pipe, glyph_pipe) = pipelines.get_pipelines(format);
    let occluder_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Kitty background occluders"),
        contents: bytemuck::cast_slice(image_occluders),
        usage: wgpu::BufferUsages::VERTEX,
    });
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
    rpass.set_pipeline(bg_pipe);
    rpass.set_bind_group(0, uniform_bg, &[]);
    rpass.set_vertex_buffer(0, bg_buf.slice(..));
    rpass.draw(0..6, 0..bg.len() as u32);
    draw_images(&mut rpass, images, glyph_pipe, uniform_bg, |z| z < i32::MIN / 2);
    if !image_occluders.is_empty() {
        rpass.set_pipeline(bg_pipe);
        rpass.set_bind_group(0, uniform_bg, &[]);
        rpass.set_vertex_buffer(0, occluder_buf.slice(..));
        rpass.draw(0..6, 0..image_occluders.len() as u32);
    }
    draw_images(&mut rpass, images, glyph_pipe, uniform_bg, |z| (i32::MIN / 2..0).contains(&z));
    if let Some(buf) = &glyph_buf {
        rpass.set_pipeline(glyph_pipe);
        rpass.set_bind_group(0, uniform_bg, &[]);
        rpass.set_bind_group(1, atlas_bg, &[]);
        rpass.set_vertex_buffer(0, buf.slice(..));
        rpass.draw(0..6, 0..glyph.len() as u32);
    }
    draw_images(&mut rpass, images, glyph_pipe, uniform_bg, |z| z >= 0);
    if let Some(buf) = &overlay_buf {
        // Surface overlays remain above every Kitty z layer.
        let overlay_pipe = pipelines.get_overlay_pipeline(format);
        rpass.set_pipeline(overlay_pipe);
        rpass.set_bind_group(0, uniform_bg, &[]);
        rpass.set_vertex_buffer(0, buf.slice(..));
        rpass.draw(0..6, 0..overlay.len() as u32);
    }
}

fn draw_images<'a>(
    pass: &mut wgpu::RenderPass<'a>, images: &'a [ImageDraw],
    pipeline: &'a wgpu::RenderPipeline, uniform: &'a wgpu::BindGroup,
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
