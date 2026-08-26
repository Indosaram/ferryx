//! Contract test for Phase 2 renderer dirty update sequence and bounded glyph atlas.

use ferryx_lib::native_terminal::{
    GlyphAtlasStats, NativeTerminalRenderer, RenderSnapshot, RendererConfig,
};

#[test]
fn test_renderer_dirty_update_sequence_and_bounded_glyph_atlas_cache() {
    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        ..Default::default()
    };

    let mut renderer = NativeTerminalRenderer::new(config).expect("renderer creation");
    let mut snapshot: RenderSnapshot = super::snapshot_builder::build_test_snapshot();

    let frame1 = renderer
        .render_snapshot(&snapshot, None)
        .expect("initial frame render");
    assert_eq!(frame1.width_px, 800);

    let initial_stats: GlyphAtlasStats = renderer.glyph_atlas_stats();
    assert!(
        initial_stats.entry_count > 0,
        "glyph atlas must populate entries for rendered glyphs"
    );
    assert!(
        initial_stats.allocated_bytes <= initial_stats.max_capacity_bytes,
        "atlas allocated bytes must not exceed max capacity"
    );

    // Sequence of dirty updates mutating cursor position and cells across repeated frames
    for i in 0..50 {
        snapshot.cursor.x = (i % 80) as u16;
        snapshot.grid[0][0].text = format!("{}", (i % 10));

        let frame = renderer
            .render_snapshot(&snapshot, None)
            .expect("dirty frame render");
        assert_eq!(frame.width_px, 800);
    }

    let final_stats: GlyphAtlasStats = renderer.glyph_atlas_stats();
    // Bounded glyph atlas: repeatedly rendering the same character set must not grow atlas unboundedly
    assert!(
        final_stats.entry_count <= initial_stats.entry_count + 15,
        "atlas entry count must remain bounded during repeated rendering"
    );
    assert!(
        final_stats.allocated_bytes <= final_stats.max_capacity_bytes,
        "atlas allocated bytes must remain within max capacity"
    );
}
