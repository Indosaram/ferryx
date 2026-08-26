//! Row-level dirty cache tracking and incremental instance reuse.

use super::atlas::GlyphAtlas;
use super::instances::{build_row_instances, compute_row_hash, RowCacheEntry};
use super::pipeline::{GlyphInstance, RectInstance};
use super::types::{RendererConfig, SelectionSnapshot};
use crate::native_terminal::snapshot::RenderSnapshot;

pub struct RowCacheManager {
    entries: Vec<RowCacheEntry>,
    last_atlas_generation: u64,
}

impl RowCacheManager {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            last_atlas_generation: 0,
        }
    }

    pub fn update_and_flatten(
        &mut self,
        snapshot: &RenderSnapshot,
        selection: Option<&SelectionSnapshot>,
        config: &RendererConfig,
        atlas: &mut GlyphAtlas,
        queue: &wgpu::Queue,
    ) -> (Vec<RectInstance>, Vec<GlyphInstance>, u16, u16) {
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
                let (bg, glyph) =
                    build_row_instances(row, snapshot, selection, config, atlas, queue);
                self.entries[row as usize] = RowCacheEntry {
                    hash: row_hash,
                    bg_instances: bg,
                    glyph_instances: glyph,
                };
                rebuilt += 1;
            }
        }

        let total_cells = (snapshot.cols as usize) * (snapshot.rows as usize);
        let mut all_bg = Vec::with_capacity(total_cells + 16);
        let mut all_glyph = Vec::new();
        for entry in &self.entries {
            all_bg.extend_from_slice(&entry.bg_instances);
            all_glyph.extend_from_slice(&entry.glyph_instances);
        }

        (all_bg, all_glyph, rebuilt, reused)
    }
}
