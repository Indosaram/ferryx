use super::composition::SurfaceCompositionLayout;
use super::renderer::canonical_scenario;
use super::snapshot::{CellSnapshot, CellWide, RenderSnapshot};

pub fn snapshot_for_layout(
    layout: SurfaceCompositionLayout,
) -> (RenderSnapshot, super::renderer::SelectionSnapshot) {
    let (mut snapshot, selection) = canonical_scenario();
    let empty_cell = CellSnapshot {
        text: String::new(),
        wide: CellWide::Narrow,
        fg: None,
        bg: None,
        bold: false,
        italic: false,
        underline: false,
        inverse: false,
    };
    let cols = layout.cols as usize;
    let rows = layout.rows as usize;
    snapshot.grid.resize(rows, vec![empty_cell.clone(); cols]);
    for row in &mut snapshot.grid {
        row.resize(cols, empty_cell.clone());
    }
    snapshot.cols = layout.cols;
    snapshot.rows = layout.rows;
    (snapshot, selection)
}
