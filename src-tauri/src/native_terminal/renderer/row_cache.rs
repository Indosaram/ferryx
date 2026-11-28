//! Row-level dirty cache tracking and incremental instance reuse.

use super::atlas::{GlyphAtlas, PreparedGlyphs};
use super::instances::{build_row_instances, compute_row_hash, RowCacheEntry};
use super::pipeline::{GlyphInstance, RectInstance};
use super::types::{RendererConfig, SelectionSnapshot};
use crate::native_terminal::error::NativeTerminalError;
use crate::native_terminal::snapshot::RenderSnapshot;

#[derive(Default)]
pub struct RowCacheManager {
    entries: Vec<RowCacheEntry>,
    last_atlas_generation: u64,
    scratch_bg: Vec<RectInstance>,
    scratch_glyph: Vec<GlyphInstance>,
}

impl RowCacheManager {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            last_atlas_generation: 0,
            scratch_bg: Vec::new(),
            scratch_glyph: Vec::new(),
        }
    }

    pub fn bg_instances(&self) -> &[RectInstance] {
        &self.scratch_bg
    }

    pub fn glyph_instances(&self) -> &[GlyphInstance] {
        &self.scratch_glyph
    }

    pub fn glyph_instances_mut(&mut self) -> &mut [GlyphInstance] {
        &mut self.scratch_glyph
    }

    pub fn update_and_flatten(
        &mut self,
        snapshot: &RenderSnapshot,
        selection: Option<&SelectionSnapshot>,
        config: &RendererConfig,
        atlas: &mut GlyphAtlas,
        queue: &wgpu::Queue,
        prepared: Option<&PreparedGlyphs>,
    ) -> Result<(u16, u16), NativeTerminalError> {
        if atlas.generation != self.last_atlas_generation {
            self.entries.clear();
            self.last_atlas_generation = atlas.generation;
        }

        if self.entries.len() != snapshot.rows as usize {
            self.entries = vec![RowCacheEntry::default(); snapshot.rows as usize];
        }

        let mut rebuilt: u16 = 0;
        let mut reused: u16 = 0;

        for row in 0..snapshot.rows {
            let row_hash = compute_row_hash(row, snapshot, selection, config);
            let cached = &self.entries[row as usize];

            if cached.hash == row_hash && !cached.bg_instances.is_empty() {
                reused += 1;
            } else {
                let (bg, glyph) = match build_row_instances(
                    row, snapshot, selection, config, atlas, queue, prepared,
                ) {
                    Ok(instances) => instances,
                    Err(error) => {
                        // Earlier rows in this attempt may already have been cached.
                        self.entries.clear();
                        return Err(error);
                    }
                };
                self.entries[row as usize] = RowCacheEntry {
                    hash: row_hash,
                    bg_instances: bg,
                    glyph_instances: glyph,
                };
                rebuilt += 1;
            }
        }

        let total_bg: usize = self.entries.iter().map(|e| e.bg_instances.len()).sum();
        let total_glyph: usize = self.entries.iter().map(|e| e.glyph_instances.len()).sum();
        self.scratch_bg.clear();
        self.scratch_bg.reserve(total_bg);
        self.scratch_glyph.clear();
        self.scratch_glyph.reserve(total_glyph);
        for entry in &self.entries {
            self.scratch_bg.extend_from_slice(&entry.bg_instances);
            self.scratch_glyph.extend_from_slice(&entry.glyph_instances);
        }

        Ok((rebuilt, reused))
    }
}
