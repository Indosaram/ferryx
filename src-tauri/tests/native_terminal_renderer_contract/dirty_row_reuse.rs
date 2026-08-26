//! Contract test for Phase 2 renderer dirty row incremental rebuild and row reuse.

use ferryx_lib::native_terminal::{NativeTerminalRenderer, RenderSnapshot, RendererConfig};

#[test]
fn test_single_row_mutation_yields_one_rebuilt_dirty_row_and_reused_rows() {
    let config = RendererConfig {
        cell_width_px: 10,
        cell_height_px: 20,
        device_scale_factor: 1.0,
        ..Default::default()
    };

    let mut renderer = NativeTerminalRenderer::new(config).expect("renderer creation");
    let mut snapshot: RenderSnapshot = super::snapshot_builder::build_test_snapshot();

    // Initial render of 80x24 grid (24 rows total)
    let initial_frame = renderer
        .render_snapshot(&snapshot, None)
        .expect("initial render snapshot");
    assert_eq!(initial_frame.rendered_row_count, 24);

    // Modify a single cell and cursor position confined within row 0
    snapshot.cursor.x = 11;
    snapshot.cursor.y = 0;
    snapshot.grid[0][0].text = "Z".to_string();

    let incremental_frame = renderer
        .render_snapshot(&snapshot, None)
        .expect("incremental render snapshot");

    assert_eq!(
        incremental_frame.rebuilt_row_count, 1,
        "single-row cell/cursor mutation must rebuild exactly 1 dirty row"
    );
    assert!(
        incremental_frame.reused_row_count >= 23,
        "single-row mutation on 24-row grid must reuse at least 23 rows (got {})",
        incremental_frame.reused_row_count
    );
}
