//! Ferryx Native Terminal WGPU Renderer Implementation.

use wgpu::util::DeviceExt;

use super::atlas::GlyphAtlas;
use super::gpu_context::GpuContext;
use super::instances::prepare_visible_glyphs;
use super::pass::encode_terminal_passes_with_surface_options;
use super::pipeline::{GlyphInstance, RectInstance, RenderPipelines, ScreenUniform};
use super::render_target::{RenderTarget, TARGET_FORMAT};
use super::row_cache::RowCacheManager;
use super::types::{GlyphAtlasStats, OffscreenFrame, RendererConfig, SelectionSnapshot};
use crate::native_terminal::composition::PhysicalBounds;
use crate::native_terminal::error::NativeTerminalError;
use crate::native_terminal::scroll::{
    compute_attention_frame_rects, compute_scrollbar_overlay_rect, ScrollbarOverlayState,
    ATTENTION_FRAME_THICKNESS_LOGICAL_PX,
};
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
    #[cfg(test)]
    preparation_attempts: usize,
    #[cfg(test)]
    fail_rebuild_after: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_terminal::cursor::{CursorSnapshot, CursorVisualStyle};
    use crate::native_terminal::renderer::rasterizer::rasterize_glyph_with_scale;
    use crate::native_terminal::snapshot::{CellSnapshot, CellWide};

    fn dense_config() -> RendererConfig {
        RendererConfig {
            cell_width_px: 16,
            cell_height_px: 32,
            device_scale_factor: 2.0,
            theme: super::super::types::RendererTheme {
                background: [0.0, 0.0, 0.0, 1.0],
                foreground: [1.0; 4],
                ..Default::default()
            },
        }
    }

    fn hangul(index: u32) -> String {
        char::from_u32(0xAC00 + index).unwrap().to_string()
    }

    fn dense_snapshot(cols: u16, rows: u16, start: u32, count: usize) -> RenderSnapshot {
        let mut grid = vec![vec![CellSnapshot::default(); cols as usize]; rows as usize];
        for i in 0..count {
            let row = i / (cols as usize / 2);
            let col = (i % (cols as usize / 2)) * 2;
            grid[row][col] = CellSnapshot {
                text: hangul(start + i as u32),
                wide: CellWide::Wide,
                ..Default::default()
            };
            grid[row][col + 1].wide = CellWide::SpacerTail;
        }
        RenderSnapshot {
            cols,
            rows,
            grid,
            cursor: CursorSnapshot {
                x: 0,
                y: 0,
                visible: false,
                blinking: false,
                wide_tail: false,
                visual_style: CursorVisualStyle::Block,
            },
        }
    }

    fn save_frame(name: &str, frame: &OffscreenFrame) {
        if let Some(dir) = std::env::var_os("ATLAS_EVIDENCE_DIR") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            frame.save_png(dir.join(format!("{name}.png"))).unwrap();
            std::fs::write(dir.join(format!("{name}.rgba")), &frame.pixels).unwrap();
            std::fs::write(
                dir.join(format!("{name}.txt")),
                format!(
                    "RGBA8 {}x{} rebuilt={} reused={}\n",
                    frame.width_px,
                    frame.height_px,
                    frame.rebuilt_row_count,
                    frame.reused_row_count
                ),
            )
            .unwrap();
        }
    }

    fn ink_origins(frame: &OffscreenFrame, cols: u16, count: usize) -> Vec<usize> {
        (0..count)
            .filter(|&i| {
                let x = (i % (cols as usize / 2)) * 32;
                let y = (i / (cols as usize / 2)) * 32;
                (y..y + 32).any(|py| {
                    (x..x + 32).any(|px| frame.pixels[(py * frame.width_px as usize + px) * 4] != 0)
                })
            })
            .collect()
    }

    #[test]
    fn all_visible_glyphs_survive_when_dense_2x_working_set_repeats() {
        // Given: 1000 distinct covered wide glyphs, not whitespace or tofu empties.
        let mut renderer = NativeTerminalRenderer::new(dense_config()).expect("real GPU");
        println!("adapter={:?}", renderer.adapter_info());
        let snapshot = dense_snapshot(80, 25, 0, 1000);
        for i in 0..1000 {
            assert!(
                rasterize_glyph_with_scale(&hangul(i), 32, 32, false, false, 2.0)
                    .buffer()
                    .iter()
                    .any(|&b| b != 0),
                "fixture ink {i}"
            );
        }
        // When: render both frames before asserting so RED observes the repeated omission.
        let first = renderer.render_snapshot(&snapshot, None).unwrap();
        let second = renderer.render_snapshot(&snapshot, None).unwrap();
        save_frame("dense-f0", &first);
        save_frame("dense-f1", &second);
        let origins = [
            ink_origins(&first, 80, 1000),
            ink_origins(&second, 80, 1000),
        ];
        println!(
            "ink origins F0={} F1={} stats={:?}",
            origins[0].len(),
            origins[1].len(),
            renderer.glyph_atlas_stats()
        );
        // Then: every origin has ink in both actual GPU readbacks, with row reuse on F1.
        for observed in origins {
            assert_eq!(observed, (0..1000).collect::<Vec<_>>());
        }
        assert_eq!(renderer.glyph_atlas_stats().entry_count, 1000);
        assert_eq!((second.rebuilt_row_count, second.reused_row_count), (0, 25));
        assert_eq!(first.pixels, second.pixels);
    }

    fn tile(frame: &OffscreenFrame, x: usize, y: usize, width: usize, height: usize) -> Vec<u8> {
        (y..y + height)
            .flat_map(|row| {
                let start = (row * frame.width_px as usize + x) * 4;
                frame.pixels[start..start + width * 4].iter().copied()
            })
            .collect()
    }

    #[test]
    fn dense_working_set_grows_across_multiple_atlas_sizes() {
        // Given: 4000 keys exceed both 1024 (992 slots) and 2048 (3968 slots).
        let mut renderer = NativeTerminalRenderer::new(dense_config()).unwrap();
        let snapshot = dense_snapshot(160, 50, 0, 4000);
        // When: the actual offscreen entry prepares and submits two frames.
        let first = renderer.render_snapshot(&snapshot, None).unwrap();
        let second = renderer.render_snapshot(&snapshot, None).unwrap();
        save_frame("dense4000-f0", &first);
        save_frame("dense4000-f1", &second);
        // Then: one allocation jumps directly to 4096, and every full tile is correct.
        assert_eq!(renderer.atlas.dimension, 4096);
        assert_eq!(renderer.atlas.growth_count, 1);
        assert_eq!(renderer.glyph_atlas_stats().entry_count, 4000);
        assert_eq!((second.rebuilt_row_count, second.reused_row_count), (0, 50));
        assert_eq!(first.pixels, second.pixels);
        for start in (0..4000).step_by(640) {
            let count = (4000 - start).min(640);
            let mut reference = NativeTerminalRenderer::new(dense_config()).unwrap();
            let batch = reference
                .render_snapshot(&dense_snapshot(160, 8, start as u32, count), None)
                .unwrap();
            assert_eq!(
                reference.atlas.dimension, 1024,
                "independent batch fits base"
            );
            save_frame(&format!("dense4000-reference-{start}"), &batch);
            for i in 0..count {
                let expected = tile(&batch, (i % 80) * 32, (i / 80) * 32, 32, 32);
                assert!(
                    expected.chunks_exact(4).any(|p| p[0] != 0),
                    "reference ink {}",
                    start + i
                );
                let actual = tile(
                    &first,
                    ((start + i) % 80) * 32,
                    ((start + i) / 80) * 32,
                    32,
                    32,
                );
                assert_eq!(actual, expected, "full tile {}", start + i);
            }
        }
        println!(
            "4000 exact full tiles; dimension={} growths={} stats={:?}",
            renderer.atlas.dimension,
            renderer.atlas.growth_count,
            renderer.glyph_atlas_stats()
        );
    }

    #[test]
    fn oversized_glyph_is_rejected_before_shelf_placement() {
        // Given: each allocation extent independently exceeds the controlled cap.
        for (cell_width_px, cell_height_px) in [(600, 32), (16, 1200)] {
            let config = RendererConfig {
                cell_width_px,
                cell_height_px,
                ..dense_config()
            };
            let snapshot = dense_snapshot(2, 1, 0, 1);
            let mut renderer = NativeTerminalRenderer::new(config).unwrap();
            renderer.atlas.max_dimension = 1024;
            // When/Then: a real render rejects before texture upload/target creation.
            assert!(matches!(
                renderer.render_snapshot(&snapshot, None),
                Err(NativeTerminalError::LimitExceeded)
            ));
            assert_eq!(renderer.atlas.stats(&config).entry_count, 0);
            assert!(renderer.target.is_none());
            renderer.gpu.check_error().unwrap();
            // The final non-power-of-two candidate is also considered, not skipped.
            renderer.atlas.max_dimension = 1500;
            let actual = renderer.render_snapshot(&snapshot, None).unwrap();
            let mut fresh = NativeTerminalRenderer::new(config).unwrap();
            let expected = fresh.render_snapshot(&snapshot, None).unwrap();
            assert!(expected.pixels.chunks_exact(4).any(|p| p[0] != 0));
            assert_eq!(renderer.atlas.dimension, 1500);
            assert_eq!(actual.pixels, expected.pixels);
            save_frame(
                &format!("oversized-{cell_width_px}-{cell_height_px}"),
                &actual,
            );
        }
    }

    #[test]
    fn empty_rasters_do_not_force_atlas_growth_under_pressure() {
        // Given: full historical shelves and 1000 visible keys, only 992 with ink.
        let config = dense_config();
        let mut renderer = NativeTerminalRenderer::new(config).unwrap();
        renderer.atlas.max_dimension = 1024;
        for i in 0..992 {
            assert!(renderer
                .atlas
                .get_or_insert(&hangul(i), false, false, true, &config, &renderer.gpu.queue)
                .unwrap()
                .is_some());
        }
        let mut snapshot = dense_snapshot(80, 25, 0, 1000);
        for i in 991..999 {
            let text = char::from_u32(0x10FFF0 + (i - 991) as u32)
                .unwrap()
                .to_string();
            assert!(!text.chars().all(char::is_whitespace));
            let raster = rasterize_glyph_with_scale(&text, 32, 32, false, false, 2.0);
            assert!(!raster.is_color());
            assert!(
                raster.buffer().iter().all(|&b| b == 0),
                "fixture must be empty"
            );
            snapshot.grid[i / 40][(i % 40) * 2].text = text;
        }
        // When: the last new covered key forces recovery with the empty classifications.
        let frame = renderer.render_snapshot(&snapshot, None).unwrap();
        let mut fresh = NativeTerminalRenderer::new(config).unwrap();
        let reference = fresh.render_snapshot(&snapshot, None).unwrap();
        // Then: no false cap failure or growth; every covered and empty pixel matches.
        assert_eq!(renderer.atlas.dimension, 1024);
        assert_eq!(renderer.atlas.growth_count, 0);
        assert_eq!(renderer.preparation_attempts, 2);
        assert_eq!(renderer.glyph_atlas_stats().entry_count, 992);
        assert_eq!(ink_origins(&reference, 80, 1000).len(), 992);
        assert_eq!(frame.pixels, reference.pixels);
        save_frame("empty-pressure", &frame);
        save_frame("empty-reference", &reference);
    }

    #[test]
    fn failed_frame_cannot_be_reused_as_partial_success() {
        let snapshot = dense_snapshot(80, 25, 0, 1000);
        let small = dense_snapshot(2, 1, 11171, 1);
        let mut fresh = NativeTerminalRenderer::new(dense_config()).unwrap();
        let reference = fresh.render_snapshot(&small, None).unwrap();
        assert_eq!(ink_origins(&reference, 2, 1), vec![0]);
        for second_pass_fault in [false, true] {
            // Given: either a true cap or an unexpected Full after a complete cached row.
            let mut renderer = NativeTerminalRenderer::new(dense_config()).unwrap();
            if second_pass_fault {
                renderer.fail_rebuild_after = Some(40);
            } else {
                renderer.atlas.max_dimension = 1024;
            }
            // When/Then: identical failed inputs remain typed errors, never cache hits.
            for _ in 0..3 {
                assert!(matches!(
                    renderer.render_snapshot(&snapshot, None),
                    Err(NativeTerminalError::LimitExceeded)
                ));
                assert_eq!(
                    renderer.preparation_attempts,
                    if second_pass_fault { 2 } else { 1 }
                );
                assert!(
                    renderer.target.is_none(),
                    "no encoding target for failed frame"
                );
            }
            renderer.fail_rebuild_after = None;
            renderer.atlas.fail_after_insertions = None;
            let recovered = renderer.render_snapshot(&small, None).unwrap();
            assert_eq!(recovered.pixels, reference.pixels);
            save_frame(&format!("failure-recovery-{second_pass_fault}"), &recovered);
            println!(
                "repeated failures fault={second_pass_fault}; smaller recovery exact; stats={:?}",
                renderer.glyph_atlas_stats()
            );
        }
    }

    #[test]
    fn surface_entry_paths_complete_dense_frames_before_submission() {
        let snapshot = dense_snapshot(80, 25, 0, 1000);
        let mut reference = NativeTerminalRenderer::new(dense_config()).unwrap();
        let expected = reference.render_snapshot(&snapshot, None).unwrap();
        // Given: fresh renderers, so each public surface entry must recover itself.
        for viewport_path in [false, true] {
            let mut renderer = NativeTerminalRenderer::new(dense_config()).unwrap();
            let (x, y) = if viewport_path { (32, 64) } else { (0, 0) };
            let (width, height) = (1280 + x * 2, 800 + y * 2);
            let target = RenderTarget::new(&renderer.gpu.device, width, height);
            // When: call the actual public API, then copy its entire attachment.
            if viewport_path {
                renderer
                    .render_to_surface_viewport(
                        &snapshot,
                        None,
                        &target.view,
                        width,
                        height,
                        TARGET_FORMAT,
                        PhysicalBounds {
                            x,
                            y,
                            width: 1280,
                            height: 800,
                        },
                        None,
                        false,
                    )
                    .unwrap();
            } else {
                renderer
                    .render_to_surface_view(
                        &snapshot,
                        None,
                        &target.view,
                        width,
                        height,
                        TARGET_FORMAT,
                    )
                    .unwrap();
            }
            let mut encoder = renderer
                .gpu
                .device
                .create_command_encoder(&Default::default());
            target.copy_to_staging(&mut encoder);
            renderer.gpu.queue.submit(Some(encoder.finish()));
            let actual = target.readback_frame(&renderer.gpu.device, 25).unwrap();
            renderer.gpu.check_error().unwrap();
            // Then: all terminal pixels equal the independently rendered offscreen frame.
            assert_eq!(renderer.atlas.growth_count, 1);
            assert_eq!(
                tile(&actual, x as usize, y as usize, 1280, 800),
                expected.pixels
            );
            for py in 0..height {
                for px in 0..width {
                    if px < x || px >= x + 1280 || py < y || py >= y + 800 {
                        assert_eq!(
                            tile(&actual, px as usize, py as usize, 1, 1),
                            [0, 0, 0, 255]
                        );
                    }
                }
            }
            save_frame(&format!("surface-viewport-{viewport_path}"), &actual);
        }
    }

    fn assert_payload(renderer: &NativeTerminalRenderer, label: &str) {
        let atlas = &renderer.atlas;
        let config = renderer.config();
        let stats = renderer.glyph_atlas_stats();
        let texture_bytes = (atlas.mask_texture.width() as usize
            * atlas.mask_texture.height() as usize
            + atlas.color_texture.width() as usize * atlas.color_texture.height() as usize)
            * 4;
        let pair_bytes = std::mem::size_of::<(
            super::super::atlas::GlyphKey,
            super::super::atlas::AtlasEntry,
        )>();
        let limit = atlas.max_dimension as usize;
        let slots = (limit / config.cell_height_px as usize)
            * ((limit + 1) / (config.cell_width_px as usize + 1));
        assert_eq!(
            stats.allocated_bytes,
            texture_bytes + stats.entry_count * pair_bytes
        );
        assert_eq!(
            stats.max_capacity_bytes,
            limit * limit * 8 + slots * pair_bytes
        );
        assert!(stats.allocated_bytes <= stats.max_capacity_bytes);
        println!("payload {label}: device_limit={limit} cells={}x{} pair_bytes={pair_bytes} texture_bytes={texture_bytes} {stats:?}", config.cell_width_px, config.cell_height_px);
    }

    #[test]
    fn tracked_payload_matches_textures_entries_clear_and_config_changes() {
        let config = dense_config();
        let mut renderer = NativeTerminalRenderer::new(config).unwrap();
        assert_payload(&renderer, "base");
        renderer
            .render_snapshot(&dense_snapshot(80, 25, 0, 1000), None)
            .unwrap();
        assert_payload(&renderer, "grown");
        renderer.atlas.clear();
        assert_eq!(renderer.glyph_atlas_stats().entry_count, 0);
        assert_payload(&renderer, "cleared");
        let small = dense_snapshot(2, 1, 11171, 1);
        for (label, updated) in [
            (
                "scale1",
                RendererConfig {
                    cell_width_px: 8,
                    cell_height_px: 16,
                    device_scale_factor: 1.0,
                    ..config
                },
            ),
            ("scale2", config),
        ] {
            renderer.update_config(updated).unwrap();
            assert_eq!(renderer.glyph_atlas_stats().entry_count, 0);
            assert_payload(&renderer, label);
            let frame = renderer.render_snapshot(&small, None).unwrap();
            let mut fresh = NativeTerminalRenderer::new(updated).unwrap();
            let reference = fresh.render_snapshot(&small, None).unwrap();
            assert!(reference.pixels.chunks_exact(4).any(|p| p[0] != 0));
            assert_eq!(frame.pixels, reference.pixels);
            assert_payload(&renderer, "repopulated");
            save_frame(label, &frame);
        }
    }

    #[test]
    fn last_frame_renders_new_glyph_when_only_history_fills_atlas() {
        // Given: exactly 992 resident covered wide keys, without requesting overflow.
        let config = dense_config();
        let mut warmed = NativeTerminalRenderer::new(config).expect("real GPU");
        for i in 0..992 {
            assert!(
                warmed
                    .atlas
                    .get_or_insert(&hangul(i), false, false, true, &config, &warmed.gpu.queue)
                    .unwrap()
                    .is_some(),
                "prewarm {i}"
            );
        }
        assert_eq!(warmed.glyph_atlas_stats().entry_count, 992);
        let snapshot = dense_snapshot(2, 1, 11171, 1);
        let mut fresh = NativeTerminalRenderer::new(config).expect("reference GPU");
        let reference = fresh.render_snapshot(&snapshot, None).unwrap();
        assert_eq!(
            ink_origins(&reference, 2, 1),
            vec![0],
            "fresh reference ink"
        );
        // When: exactly one final render; no unrelated repaint is permitted.
        let final_frame = warmed.render_snapshot(&snapshot, None).unwrap();
        save_frame("history-final", &final_frame);
        save_frame("history-reference", &reference);
        // Then: the complete glyph-cell readback equals the independent fresh frame.
        assert_eq!(final_frame.pixels, reference.pixels);
    }
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
            #[cfg(test)]
            preparation_attempts: 0,
            #[cfg(test)]
            fail_rebuild_after: None,
        })
    }

    /// No caller may encode or submit until this returns a complete set of rows.
    fn prepare_frame_instances(
        &mut self,
        snapshot: &RenderSnapshot,
        selection: Option<&SelectionSnapshot>,
    ) -> Result<(Vec<RectInstance>, Vec<GlyphInstance>, u16, u16), NativeTerminalError> {
        #[cfg(test)]
        {
            self.preparation_attempts = 1;
        }
        match self.row_cache.update_and_flatten(
            snapshot,
            selection,
            &self.config,
            &mut self.atlas,
            &self.gpu.queue,
            None,
        ) {
            Ok(instances) => return Ok(instances),
            Err(NativeTerminalError::LimitExceeded) => {}
            Err(error) => return Err(error),
        }

        // Only pressure pays for a row-major first-occurrence raster sequence.
        // Fit and the one permitted rebuild use the SAME dimensions and empties.
        let prepared = prepare_visible_glyphs(snapshot, &self.config);
        let dimension =
            prepared.fitting_dimension(self.atlas.dimension, self.atlas.max_dimension)?;
        let grew = dimension != self.atlas.dimension;
        self.atlas.reset_for_rebuild(&self.gpu.device, dimension);
        if grew {
            self.atlas_bind_group = self
                .gpu
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Atlas Bind Group"),
                    layout: &self.pipelines.atlas_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&self.atlas.mask_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&self.atlas.color_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.atlas.sampler),
                        },
                    ],
                });
        }
        #[cfg(test)]
        {
            self.preparation_attempts = 2;
            self.atlas.fail_after_insertions = self.fail_rebuild_after;
        }
        // The row cache invalidates ALL partial rows on every error. In particular,
        // a second Full remains LimitExceeded: no encode, partial success or third try.
        self.row_cache.update_and_flatten(
            snapshot,
            selection,
            &self.config,
            &mut self.atlas,
            &self.gpu.queue,
            Some(&prepared),
        )
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

    /// Presents a surface texture using the GPU queue.
    pub fn present(&self, frame: wgpu::SurfaceTexture) {
        self.gpu.queue.present(frame);
    }

    pub fn render_snapshot(
        &mut self,
        snapshot: &RenderSnapshot,
        selection: Option<&SelectionSnapshot>,
    ) -> Result<OffscreenFrame, NativeTerminalError> {
        let (width_px, height_px) = self.validate_and_dims(snapshot)?;
        let (bg, glyph, rebuilt, reused) = self.prepare_frame_instances(snapshot, selection)?;

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
            &[],
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
        let (bg, glyph, rebuilt, reused) = self.prepare_frame_instances(snapshot, selection)?;

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
            &[],
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
        scrollbar_overlay: Option<&ScrollbarOverlayState>,
        attention_frame: bool,
    ) -> Result<(u16, u16), NativeTerminalError> {
        self.render_to_surface_viewport_internal(
            snapshot,
            selection,
            view,
            surface_width_px,
            surface_height_px,
            format,
            viewport,
            scrollbar_overlay,
            attention_frame,
            false,
        )
    }

    fn render_to_surface_viewport_internal(
        &mut self,
        snapshot: &RenderSnapshot,
        selection: Option<&SelectionSnapshot>,
        view: &wgpu::TextureView,
        surface_width_px: u32,
        surface_height_px: u32,
        format: wgpu::TextureFormat,
        viewport: PhysicalBounds,
        scrollbar_overlay: Option<&ScrollbarOverlayState>,
        attention_frame: bool,
        include_bottom_band: bool,
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

        let (bg, mut glyph, rebuilt, reused) = self.prepare_frame_instances(snapshot, selection)?;

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

        let mut overlay_instances = Vec::new();
        if let Some(overlay) = scrollbar_overlay {
            if overlay.visible {
                if let Some(metrics) = overlay.metrics {
                    if let Some(rect) = compute_scrollbar_overlay_rect(
                        viewport,
                        metrics.total,
                        metrics.offset,
                        metrics.len,
                        self.config.theme.foreground,
                    ) {
                        overlay_instances.push(rect);
                    }
                }
            }
        }
        if attention_frame {
            let thickness = ATTENTION_FRAME_THICKNESS_LOGICAL_PX * self.config.device_scale_factor;
            overlay_instances.extend(compute_attention_frame_rects(
                viewport,
                thickness,
                include_bottom_band,
            ));
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
            &overlay_instances,
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
        self.render_to_offscreen_viewport_with_overlay(
            snapshot,
            selection,
            surface_width_px,
            surface_height_px,
            viewport,
            None,
        )
    }

    /// Renders snapshot passes with an optional scrollbar overlay to an offscreen viewport target.
    pub fn render_to_offscreen_viewport_with_overlay(
        &mut self,
        snapshot: &RenderSnapshot,
        selection: Option<&SelectionSnapshot>,
        surface_width_px: u32,
        surface_height_px: u32,
        viewport: PhysicalBounds,
        scrollbar_overlay: Option<&ScrollbarOverlayState>,
    ) -> Result<OffscreenFrame, NativeTerminalError> {
        self.render_to_offscreen_viewport_with_overlay_and_attention(
            snapshot,
            selection,
            surface_width_px,
            surface_height_px,
            viewport,
            scrollbar_overlay,
            false,
        )
    }

    /// Renders snapshot passes with an optional scrollbar overlay and attention frame to an offscreen viewport target.
    pub fn render_to_offscreen_viewport_with_overlay_and_attention(
        &mut self,
        snapshot: &RenderSnapshot,
        selection: Option<&SelectionSnapshot>,
        surface_width_px: u32,
        surface_height_px: u32,
        viewport: PhysicalBounds,
        scrollbar_overlay: Option<&ScrollbarOverlayState>,
        attention_frame: bool,
    ) -> Result<OffscreenFrame, NativeTerminalError> {
        let target = RenderTarget::new(&self.gpu.device, surface_width_px, surface_height_px);
        let (rebuilt, reused) = self.render_to_surface_viewport_internal(
            snapshot,
            selection,
            &target.view,
            surface_width_px,
            surface_height_px,
            TARGET_FORMAT,
            viewport,
            scrollbar_overlay,
            attention_frame,
            true,
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
        self.atlas.stats(&self.config)
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
