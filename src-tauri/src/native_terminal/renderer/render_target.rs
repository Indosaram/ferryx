//! Offscreen texture render target and staging buffer readback logic.

use super::types::OffscreenFrame;
use crate::native_terminal::error::NativeTerminalError;

pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct RenderTarget {
    pub width: u32,
    pub height: u32,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub staging_buffer: wgpu::Buffer,
    pub padded_bytes_per_row: u32,
}

impl RenderTarget {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Offscreen Target Texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let unpadded_bytes_per_row = width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = (unpadded_bytes_per_row + align - 1) & !(align - 1);
        let buffer_size = (padded_bytes_per_row * height) as u64;

        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Offscreen Staging Readback Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            width,
            height,
            texture,
            view,
            staging_buffer,
            padded_bytes_per_row,
        }
    }

    pub fn copy_to_staging(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn readback_frame(
        &self,
        device: &wgpu::Device,
        rendered_row_count: u16,
    ) -> Result<OffscreenFrame, NativeTerminalError> {
        let slice = self.staging_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();

        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });

        device.poll(wgpu::Maintain::Wait);

        rx.recv()
            .map_err(|e| NativeTerminalError::GpuBufferError(e.to_string()))?
            .map_err(|e| NativeTerminalError::GpuBufferError(e.to_string()))?;

        let mut pixels = Vec::with_capacity((self.width * self.height * 4) as usize);
        {
            let mapped = slice.get_mapped_range();
            for row in 0..self.height {
                let start = (row * self.padded_bytes_per_row) as usize;
                let end = start + (self.width * 4) as usize;
                pixels.extend_from_slice(&mapped[start..end]);
            }
        }
        self.staging_buffer.unmap();

        Ok(OffscreenFrame {
            width_px: self.width,
            height_px: self.height,
            pixels,
            rendered_row_count,
            rebuilt_row_count: 0,
            reused_row_count: 0,
        })
    }
}
