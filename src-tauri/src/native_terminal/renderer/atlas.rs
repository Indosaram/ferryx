//! Bounded glyph atlas texture management and UV coordinates cache.

use std::collections::HashMap;

use super::rasterizer::rasterize_glyph_with_scale;
use super::types::{GlyphAtlasStats, RendererConfig};

const ATLAS_WIDTH: u32 = 512;
const ATLAS_HEIGHT: u32 = 512;
const MAX_CAPACITY_BYTES: usize = 4 * 1024 * 1024; // 4 MB

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AtlasEntry {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub width: u32,
    pub height: u32,
}

pub struct GlyphAtlas {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub generation: u64,
    entries: HashMap<GlyphKey, AtlasEntry>,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
}

impl GlyphAtlas {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Ferryx Glyph Atlas Texture"),
            size: wgpu::Extent3d {
                width: ATLAS_WIDTH,
                height: ATLAS_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Ferryx Glyph Atlas Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            texture,
            view,
            sampler,
            generation: 0,
            entries: HashMap::new(),
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
        }
    }

    pub fn get_or_insert(
        &mut self,
        text: &str,
        bold: bool,
        italic: bool,
        is_wide: bool,
        config: &RendererConfig,
        queue: &wgpu::Queue,
    ) -> Option<AtlasEntry> {
        if text.is_empty() || text.chars().all(|c| c.is_whitespace()) {
            return None;
        }

        let key = GlyphKey {
            text: text.to_string(),
            bold,
            italic,
        };

        if let Some(&entry) = self.entries.get(&key) {
            return Some(entry);
        }

        let width = if is_wide {
            config.cell_width_px * 2
        } else {
            config.cell_width_px
        };
        let height = config.cell_height_px;

        if self.cursor_x + width > ATLAS_WIDTH {
            self.cursor_x = 0;
            self.cursor_y += self.row_height;
            self.row_height = 0;
        }

        if self.cursor_y + height > ATLAS_HEIGHT {
            // Capacity boundary reached: clear and reset
            self.entries.clear();
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.row_height = 0;
            self.generation = self.generation.wrapping_add(1);
        }

        let alpha_bytes = rasterize_glyph_with_scale(
            text,
            width,
            height,
            bold,
            italic,
            config.device_scale_factor,
        );

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: self.cursor_x,
                    y: self.cursor_y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &alpha_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let uv_min = [
            self.cursor_x as f32 / ATLAS_WIDTH as f32,
            self.cursor_y as f32 / ATLAS_HEIGHT as f32,
        ];
        let uv_max = [
            (self.cursor_x + width) as f32 / ATLAS_WIDTH as f32,
            (self.cursor_y + height) as f32 / ATLAS_HEIGHT as f32,
        ];

        let entry = AtlasEntry {
            uv_min,
            uv_max,
            width,
            height,
        };

        self.cursor_x += width + 1;
        self.row_height = self.row_height.max(height);
        self.entries.insert(key, entry);

        Some(entry)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.row_height = 0;
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn stats(&self) -> GlyphAtlasStats {
        let entry_overhead = self.entries.len() * std::mem::size_of::<(GlyphKey, AtlasEntry)>();
        let allocated_bytes = (ATLAS_WIDTH * ATLAS_HEIGHT) as usize + entry_overhead;

        GlyphAtlasStats {
            entry_count: self.entries.len(),
            allocated_bytes,
            max_capacity_bytes: MAX_CAPACITY_BYTES,
        }
    }
}
