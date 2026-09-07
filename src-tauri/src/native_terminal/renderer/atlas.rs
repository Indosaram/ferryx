//! Bounded glyph atlas texture management and UV coordinates cache.

use std::collections::HashMap;

use super::rasterizer::{rasterize_glyph_with_scale, RasterizedGlyph};
use super::types::{GlyphAtlasStats, RendererConfig};
use crate::native_terminal::error::NativeTerminalError;

const BASE_DIMENSION: u32 = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AtlasEntry {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub width: u32,
    pub height: u32,
    pub is_color: bool,
}

/// One rasterization/classification retained for both fit and recovery insertion.
pub struct PreparedGlyph {
    pub key: GlyphKey,
    width: u32,
    height: u32,
    raster: Option<RasterizedGlyph>,
}

impl PreparedGlyph {
    pub fn new(key: GlyphKey, is_wide: bool, config: &RendererConfig) -> Self {
        let width = config.cell_width_px * if is_wide { 2 } else { 1 };
        let height = config.cell_height_px;
        let raster = if key.text.is_empty() || key.text.chars().all(char::is_whitespace) {
            None
        } else {
            let raster = rasterize_glyph_with_scale(
                &key.text,
                width,
                height,
                key.bold,
                key.italic,
                config.device_scale_factor,
            );
            // Color rasters retain their slots even when all bytes are zero.
            (raster.is_color() || raster.buffer().iter().any(|&b| b != 0)).then_some(raster)
        };
        Self {
            key,
            width,
            height,
            raster,
        }
    }
}

#[derive(Default)]
pub struct PreparedGlyphs {
    ordered: Vec<PreparedGlyph>,
    by_key: HashMap<GlyphKey, usize>,
}

impl PreparedGlyphs {
    pub fn add(&mut self, key: GlyphKey, is_wide: bool, config: &RendererConfig) {
        if !self.by_key.contains_key(&key) {
            self.by_key.insert(key.clone(), self.ordered.len());
            self.ordered.push(PreparedGlyph::new(key, is_wide, config));
        }
    }

    pub fn get(&self, key: &GlyphKey) -> &PreparedGlyph {
        &self.ordered[self.by_key[key]]
    }

    pub fn fitting_dimension(
        &self,
        current: u32,
        maximum: u32,
    ) -> Result<u32, NativeTerminalError> {
        let mut candidate = current.min(maximum);
        loop {
            let mut shelf = Shelf::default();
            if self
                .ordered
                .iter()
                .all(|g| g.raster.is_none() || shelf.place(g.width, g.height, candidate).is_some())
            {
                return Ok(candidate);
            }
            if candidate == maximum {
                return Err(NativeTerminalError::LimitExceeded);
            }
            candidate = candidate.saturating_mul(2).min(maximum);
        }
    }
}

/// Simulation and real uploads share this placement operation. Failure never mutates shelves.
#[derive(Clone, Copy, Default)]
struct Shelf {
    x: u32,
    y: u32,
    height: u32,
}

impl Shelf {
    fn place(&mut self, width: u32, height: u32, dimension: u32) -> Option<[u32; 2]> {
        if width > dimension || height > dimension {
            return None;
        }
        let mut next = *self;
        if next.x.checked_add(width)? > dimension {
            next.x = 0;
            next.y = next.y.checked_add(next.height)?;
            next.height = 0;
        }
        if next.y.checked_add(height)? > dimension {
            return None;
        }
        let origin = [next.x, next.y];
        next.x = next.x.checked_add(width)?.checked_add(1)?;
        next.height = next.height.max(height);
        *self = next;
        Some(origin)
    }
}

pub struct GlyphAtlas {
    pub mask_texture: wgpu::Texture,
    pub mask_view: wgpu::TextureView,
    pub color_texture: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub generation: u64,
    entries: HashMap<GlyphKey, AtlasEntry>,
    shelf: Shelf,
    pub dimension: u32,
    pub max_dimension: u32,
    #[cfg(test)]
    pub growth_count: usize,
    #[cfg(test)]
    pub fail_after_insertions: Option<usize>,
}

impl GlyphAtlas {
    pub fn new(device: &wgpu::Device) -> Self {
        Self::with_dimension(
            device,
            BASE_DIMENSION.min(device.limits().max_texture_dimension_2d),
        )
    }

    fn with_dimension(device: &wgpu::Device, dimension: u32) -> Self {
        let descriptor = wgpu::TextureDescriptor {
            label: Some("Ferryx Glyph Atlas Mask Texture"),
            size: wgpu::Extent3d {
                width: dimension,
                height: dimension,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };
        let mask_texture = device.create_texture(&descriptor);
        let mask_view = mask_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Ferryx Glyph Atlas Color Texture"),
            ..descriptor
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Ferryx Glyph Atlas Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Self {
            mask_texture,
            mask_view,
            color_texture,
            color_view,
            sampler,
            generation: 0,
            entries: HashMap::new(),
            shelf: Shelf::default(),
            dimension,
            max_dimension: device.limits().max_texture_dimension_2d,
            #[cfg(test)]
            growth_count: 0,
            #[cfg(test)]
            fail_after_insertions: None,
        }
    }

    pub fn reset_for_rebuild(&mut self, device: &wgpu::Device, dimension: u32) {
        if dimension != self.dimension {
            let mut replacement = Self::with_dimension(device, dimension);
            replacement.generation = self.generation;
            replacement.max_dimension = self.max_dimension;
            #[cfg(test)]
            {
                replacement.growth_count = self.growth_count + 1;
                replacement.fail_after_insertions = self.fail_after_insertions;
            }
            *self = replacement;
        }
        self.clear();
    }

    pub fn get_entry(&self, text: &str, bold: bool, italic: bool) -> Option<AtlasEntry> {
        let key = GlyphKey {
            text: text.to_string(),
            bold,
            italic,
        };
        self.entries.get(&key).copied()
    }

    pub fn get_or_insert(
        &mut self,
        text: &str,
        bold: bool,
        italic: bool,
        is_wide: bool,
        config: &RendererConfig,
        queue: &wgpu::Queue,
    ) -> Result<Option<AtlasEntry>, NativeTerminalError> {
        if text.is_empty() || text.chars().all(char::is_whitespace) {
            return Ok(None);
        }
        if let Some(entry) = self.get_entry(text, bold, italic) {
            return Ok(Some(entry));
        }
        let key = GlyphKey {
            text: text.to_string(),
            bold,
            italic,
        };
        self.insert_prepared(&PreparedGlyph::new(key, is_wide, config), queue)
    }

    /// Empty is successful absence; Full is typed LimitExceeded, never a missing glyph.
    pub fn insert_prepared(
        &mut self,
        glyph: &PreparedGlyph,
        queue: &wgpu::Queue,
    ) -> Result<Option<AtlasEntry>, NativeTerminalError> {
        if let Some(&entry) = self.entries.get(&glyph.key) {
            return Ok(Some(entry));
        }
        let Some(rasterized) = &glyph.raster else {
            return Ok(None);
        };
        #[cfg(test)]
        if let Some(remaining) = &mut self.fail_after_insertions {
            if *remaining == 0 {
                return Err(NativeTerminalError::LimitExceeded);
            }
            *remaining -= 1;
        }
        let (width, height) = (glyph.width, glyph.height);
        let [x, y] = self
            .shelf
            .place(width, height, self.dimension)
            .ok_or(NativeTerminalError::LimitExceeded)?;
        let is_color = rasterized.is_color();
        let alpha_rgba;
        let (texture, bytes) = match rasterized {
            RasterizedGlyph::Alpha(alpha) => {
                alpha_rgba = alpha.iter().flat_map(|&a| [a, a, a, a]).collect::<Vec<_>>();
                (&self.mask_texture, alpha_rgba.as_slice())
            }
            RasterizedGlyph::Subpixel(bytes) => (&self.mask_texture, bytes.as_slice()),
            RasterizedGlyph::Color(bytes) => (&self.color_texture, bytes.as_slice()),
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let entry = AtlasEntry {
            uv_min: [
                x as f32 / self.dimension as f32,
                y as f32 / self.dimension as f32,
            ],
            uv_max: [
                (x + width) as f32 / self.dimension as f32,
                (y + height) as f32 / self.dimension as f32,
            ],
            width,
            height,
            is_color,
        };
        self.entries.insert(glyph.key.clone(), entry);
        Ok(Some(entry))
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.shelf = Shelf::default();
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn stats(&self, config: &RendererConfig) -> GlyphAtlasStats {
        let pair_bytes = std::mem::size_of::<(GlyphKey, AtlasEntry)>();
        // Cast before byte arithmetic. The theoretical maximum saturates if a
        // device's bound exceeds the address space; current payload stays exact.
        let dimension = self.dimension as usize;
        let maximum = self.max_dimension as usize;
        let max_entries = (maximum / config.cell_height_px as usize)
            .saturating_mul(maximum.saturating_add(1) / (config.cell_width_px as usize + 1));
        GlyphAtlasStats {
            entry_count: self.entries.len(),
            allocated_bytes: dimension * dimension * 8 + self.entries.len() * pair_bytes,
            max_capacity_bytes: maximum
                .saturating_mul(maximum)
                .saturating_mul(8)
                .saturating_add(max_entries.saturating_mul(pair_bytes)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_terminal::renderer::gpu_context::GpuContext;
    use crate::native_terminal::renderer::types::RendererTheme;

    #[test]
    fn test_atlas_color_and_mask_coexist() {
        let gpu = match GpuContext::new() {
            Ok(gpu) => gpu,
            Err(_) => return,
        };

        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = RendererConfig {
            cell_width_px: 10,
            cell_height_px: 20,
            device_scale_factor: 1.0,
            theme: RendererTheme::default(),
        };

        let mask_entry = atlas
            .get_or_insert("A", false, false, false, &config, &gpu.queue)
            .unwrap()
            .expect("must insert 'A'");
        assert!(
            !mask_entry.is_color,
            "'A' should be a mask entry (not color)"
        );

        #[cfg(target_os = "macos")]
        {
            let color_entry = atlas
                .get_or_insert("😺", false, false, true, &config, &gpu.queue)
                .unwrap()
                .expect("must insert '😺'");
            assert!(color_entry.is_color, "'😺' should be a color entry");

            let stats = atlas.stats(&config);
            assert_eq!(stats.entry_count, 2, "both mask and color entries coexist");

            let queried_mask = atlas.get_entry("A", false, false).expect("find 'A'");
            assert!(!queried_mask.is_color);

            let queried_color = atlas.get_entry("😺", false, false).expect("find '😺'");
            assert!(queried_color.is_color);
        }
    }

    #[test]
    fn atlas_overflow_never_invalidates_earlier_entries_mid_frame() {
        let gpu = match GpuContext::new() {
            Ok(g) => g,
            Err(_) => return,
        };
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = RendererConfig {
            cell_width_px: 16,
            cell_height_px: 32,
            device_scale_factor: 2.0,
            theme: RendererTheme::default(),
        };
        let first = atlas
            .get_or_insert("a0", false, false, false, &config, &gpu.queue)
            .unwrap()
            .expect("first insert must succeed");
        let generation_after_first = atlas.generation;

        // Insert unique narrow glyphs far beyond capacity (unique text keys, e.g. "a1".."a3000").
        let mut saw_overflow = false;
        for i in 1..3000u32 {
            if atlas
                .get_or_insert(&format!("a{i}"), false, false, false, &config, &gpu.queue)
                .is_err()
            {
                saw_overflow = true;
                break;
            }
        }
        assert!(
            saw_overflow,
            "3000 unique 16x32 slots must exceed any atlas budget"
        );
        assert_eq!(
            atlas.generation, generation_after_first,
            "overflow must NOT bump generation mid-frame"
        );
        assert_eq!(
            atlas.get_entry("a0", false, false),
            Some(first),
            "overflow must NOT evict entries inserted earlier in the frame"
        );

        // After a between-frame clear, fresh inserts must succeed again.
        atlas.clear();
        assert!(
            atlas
                .get_or_insert("a0", false, false, false, &config, &gpu.queue)
                .unwrap()
                .is_some(),
            "insert after clear must succeed"
        );
    }

    #[test]
    fn atlas_skips_all_zero_raster_without_allocating_slot() {
        let gpu = match GpuContext::new() {
            Ok(g) => g,
            Err(_) => return,
        };
        let mut atlas = GlyphAtlas::new(&gpu.device);
        let config = RendererConfig {
            cell_width_px: 16,
            cell_height_px: 32,
            device_scale_factor: 2.0,
            theme: RendererTheme::default(),
        };
        let before = atlas.stats(&config).entry_count;
        let result = atlas.get_or_insert("\u{10FFFD}", false, false, false, &config, &gpu.queue);
        assert!(
            result.unwrap().is_none(),
            "all-zero raster must return None"
        );
        assert_eq!(
            atlas.stats(&config).entry_count,
            before,
            "all-zero raster must not allocate an atlas slot"
        );
    }
}
