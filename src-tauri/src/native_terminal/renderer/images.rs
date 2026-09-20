//! Generation-keyed image textures and clipped textured quads.
use super::{
    pipeline::{GlyphInstance, RectInstance, RenderPipelines},
    types::{RendererConfig, SelectionSnapshot},
};
use crate::native_terminal::{NativeTerminalError, RenderSnapshot};
use std::collections::{HashMap, HashSet};
use wgpu::util::DeviceExt;

#[derive(Default)]
pub struct ImageTextures {
    textures: HashMap<u64, wgpu::BindGroup>,
    #[cfg(test)]
    pub upload_count: usize,
}

pub struct ImageDraw {
    pub z: i32,
    pub binding: wgpu::BindGroup,
    pub vertices: wgpu::Buffer,
}

impl ImageTextures {
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &RenderPipelines,
        snapshot: &RenderSnapshot,
        config: &RendererConfig,
        origin: [f32; 2],
    ) -> Result<Vec<ImageDraw>, NativeTerminalError> {
        let mut retained = HashSet::new();
        let mut draws = Vec::new();
        let width = snapshot.cols as f32 * config.cell_width_px as f32;
        let height = snapshot.rows as f32 * config.cell_height_px as f32;
        for placement in &snapshot.images {
            let image = &placement.image;
            let [sx, sy, sw, sh] = placement.source;
            let x = placement.viewport_col as f32 * config.cell_width_px as f32
                + placement.offset_x as f32;
            let y = placement.viewport_row as f32 * config.cell_height_px as f32
                + placement.offset_y as f32;
            let w = placement.pixel_width as f32;
            let h = placement.pixel_height as f32;
            let left = x.max(0.0);
            let top = y.max(0.0);
            let right = (x + w).min(width);
            let bottom = (y + h).min(height);
            if right <= left || bottom <= top || sw == 0 || sh == 0 {
                continue;
            }
            if image.width > device.limits().max_texture_dimension_2d
                || image.height > device.limits().max_texture_dimension_2d
            {
                // Ghostty accepts larger images than some GPUs; keep text repainting.
                continue;
            }
            retained.insert(image.generation);
            if !self.textures.contains_key(&image.generation) {
                let expected = (image.width as usize)
                    .checked_mul(image.height as usize)
                    .and_then(|n| n.checked_mul(4));
                if image.width == 0 || image.height == 0 || expected != Some(image.rgba.len()) {
                    return Err(NativeTerminalError::LimitExceeded);
                }
                let size = wgpu::Extent3d {
                    width: image.width,
                    height: image.height,
                    depth_or_array_layers: 1,
                };
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Kitty image"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &image.rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(image.width * 4),
                        rows_per_image: Some(image.height),
                    },
                    size,
                );
                let view = texture.create_view(&Default::default());
                let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("Kitty image sampler"),
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    ..Default::default()
                });
                let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Kitty image binding"),
                    layout: &pipelines.atlas_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                    ],
                });
                self.textures.insert(image.generation, binding);
                #[cfg(test)]
                {
                    self.upload_count += 1;
                }
            }
            let u = |p: f32| (sx as f32 + p * sw as f32) / image.width as f32;
            let v = |p: f32| (sy as f32 + p * sh as f32) / image.height as f32;
            let instance = GlyphInstance {
                rect: [
                    origin[0] + left,
                    origin[1] + top,
                    right - left,
                    bottom - top,
                ],
                uv: [
                    u((left - x) / w),
                    v((top - y) / h),
                    u((right - x) / w),
                    v((bottom - y) / h),
                ],
                color: [1.0; 4],
                is_color: 2.0,
                _pad: [0.0; 3],
            };
            draws.push(ImageDraw {
                z: placement.z,
                binding: self.textures[&image.generation].clone(),
                vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Kitty image quad"),
                    contents: bytemuck::bytes_of(&instance),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            });
        }
        self.textures
            .retain(|generation, _| retained.contains(generation));
        draws.sort_by_key(|d| d.z);
        Ok(draws)
    }
}

/// Below-background images cover the default background but not explicitly
/// colored cells, selection, cursor, or decorations. Reuse the actual resolved
/// background instances so inverse/selection theme colors stay authoritative.
pub fn image_background_occluders(
    snapshot: &RenderSnapshot,
    config: &RendererConfig,
    selection: Option<&SelectionSnapshot>,
    bg: &[RectInstance],
) -> Vec<RectInstance> {
    if !snapshot.images.iter().any(|p| p.z < i32::MIN / 2) {
        return Vec::new();
    }
    bg.iter()
        .filter(|rect| {
            let col = (rect.rect[0] / config.cell_width_px as f32) as u16;
            let row = (rect.rect[1] / config.cell_height_px as f32) as u16;
            snapshot
                .cell(col as usize, row as usize)
                .is_some_and(|c| c.bg.is_some() || c.inverse)
                || selection.is_some_and(|s| s.contains_cell(col, row))
                || (snapshot.cursor.visible && snapshot.cursor.x == col && snapshot.cursor.y == row)
                || rect.rect[2] != config.cell_width_px as f32
                || rect.rect[3] != config.cell_height_px as f32
        })
        .copied()
        .collect()
}
