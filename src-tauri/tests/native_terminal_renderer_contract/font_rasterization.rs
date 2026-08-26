//! Contract tests for real font rasterization, CJK/Unicode fallback, and cell metrics derivation.

use ferryx_lib::native_terminal::renderer::font_manager::FontManager;
use fontdb::Database;

#[test]
fn test_font_rasterization_ascii_cjk_fallback_and_missing_glyph_contract() {
    // 1. System fonts test (when available on the host machine)
    let global_mgr = FontManager::global();
    let metrics = global_mgr.cell_metrics();

    // Derived cell metrics must be strictly positive and consistent with font size
    assert!(
        metrics.width_px > 0,
        "derived cell width must be non-zero (got {})",
        metrics.width_px
    );
    assert!(
        metrics.height_px > 0,
        "derived cell height must be non-zero (got {})",
        metrics.height_px
    );

    let configured_size = global_mgr.font_size();
    assert!(
        configured_size > 0.0,
        "configured font size must be positive"
    );

    // Rasterize ASCII char 'A'
    let ascii_mask =
        global_mgr.rasterize_glyph("A", metrics.width_px, metrics.height_px, false, false);
    assert_eq!(
        ascii_mask.len(),
        (metrics.width_px * metrics.height_px) as usize,
        "ASCII mask length must equal width * height"
    );

    // Rasterize CJK char '가' (wide cell: 2 * width)
    let cjk_mask =
        global_mgr.rasterize_glyph("가", metrics.width_px * 2, metrics.height_px, false, false);
    assert_eq!(
        cjk_mask.len(),
        (metrics.width_px * 2 * metrics.height_px) as usize,
        "CJK mask length must equal 2 * width * height"
    );

    // If system fonts are present, ASCII 'A' must produce non-empty alpha pixels
    let has_faces = {
        let mut db = Database::new();
        db.load_system_fonts();
        let has = db.faces().next().is_some();
        has
    };

    if has_faces {
        let ascii_non_empty = ascii_mask.iter().any(|&b| b > 0);
        assert!(
            ascii_non_empty,
            "ASCII 'A' must produce a non-empty alpha mask when system fonts exist"
        );
    }

    // 2. Empty / isolated font database test: must never panic and must return an empty alpha mask
    let empty_db = Database::new();
    let isolated_mgr = FontManager::new_with_db(empty_db, "monospace", 14.0);

    let iso_metrics = isolated_mgr.cell_metrics();
    assert!(
        iso_metrics.width_px > 0,
        "fallback cell width without fonts must still be non-zero"
    );
    assert!(
        iso_metrics.height_px > 0,
        "fallback cell height without fonts must still be non-zero"
    );

    // Missing glyph with no fonts must return an empty mask without panicking
    let isolated_ascii = isolated_mgr.rasterize_glyph("A", 10, 20, false, false);
    assert_eq!(isolated_ascii.len(), 200);
    assert!(
        isolated_ascii.iter().all(|&b| b == 0),
        "isolated manager with no faces must return an all-zero alpha mask for ASCII"
    );

    let isolated_cjk = isolated_mgr.rasterize_glyph("가", 20, 20, false, false);
    assert_eq!(isolated_cjk.len(), 400);
    assert!(
        isolated_cjk.iter().all(|&b| b == 0),
        "isolated manager with no faces must return an all-zero alpha mask for CJK"
    );

    let isolated_combining = isolated_mgr.rasterize_glyph("e\u{0301}", 10, 20, true, true);
    assert_eq!(isolated_combining.len(), 200);
    assert!(
        isolated_combining.iter().all(|&b| b == 0),
        "isolated manager with combining marks must return an all-zero mask without panic"
    );
}

#[test]
fn test_derived_cell_metrics_consistency_with_configured_font_sizes() {
    for size in [10.0, 13.0, 16.0, 24.0] {
        let mut db = Database::new();
        db.load_system_fonts();
        let mgr = FontManager::new_with_db(db, "monospace", size);
        let metrics = mgr.cell_metrics();

        assert!(
            metrics.width_px > 0,
            "size {size} must yield non-zero width"
        );
        assert!(
            metrics.height_px > 0,
            "size {size} must yield non-zero height"
        );
        assert_eq!(mgr.font_size(), size);
    }
}

#[test]
fn test_retina_scale_metrics_derivation_and_dynamic_switch() {
    let global_mgr = FontManager::global();

    let m1 = global_mgr.cell_metrics_for_scale(1.0);
    let m2 = global_mgr.cell_metrics_for_scale(2.0);

    assert!(
        m2.width_px > m1.width_px,
        "2x Retina scale width ({}) must exceed 1x width ({})",
        m2.width_px,
        m1.width_px
    );
    assert!(
        m2.height_px > m1.height_px,
        "2x Retina scale height ({}) must exceed 1x height ({})",
        m2.height_px,
        m1.height_px
    );

    let width_ratio = m2.width_px as f32 / m1.width_px as f32;
    let height_ratio = m2.height_px as f32 / m1.height_px as f32;
    assert!(
        (width_ratio - 2.0).abs() < 0.35,
        "width ratio ({width_ratio}) must scale approximately 2x"
    );
    assert!(
        (height_ratio - 2.0).abs() < 0.35,
        "height ratio ({height_ratio}) must scale approximately 2x"
    );

    // Global FontManager must remain scale-stateless and never lock into the first queried scale
    let m1_again = global_mgr.cell_metrics_for_scale(1.0);
    assert_eq!(
        m1, m1_again,
        "Querying 1x scale after 2x scale must return exact original 1x metrics"
    );
}

#[test]
fn test_retina_scale_glyph_rasterization_sharpness() {
    let global_mgr = FontManager::global();
    let m1 = global_mgr.cell_metrics_for_scale(1.0);
    let m2 = global_mgr.cell_metrics_for_scale(2.0);

    let mask_1x =
        global_mgr.rasterize_glyph_for_scale("A", m1.width_px, m1.height_px, false, false, 1.0);
    let mask_2x =
        global_mgr.rasterize_glyph_for_scale("A", m2.width_px, m2.height_px, false, false, 2.0);

    assert_eq!(
        mask_1x.len(),
        (m1.width_px * m1.height_px) as usize,
        "1x mask length matches 1x cell dimensions"
    );
    assert_eq!(
        mask_2x.len(),
        (m2.width_px * m2.height_px) as usize,
        "2x mask length matches 2x cell dimensions"
    );

    let count_1x = mask_1x.iter().filter(|&&b| b > 0).count();
    let count_2x = mask_2x.iter().filter(|&&b| b > 0).count();

    if count_1x > 0 {
        assert!(
            count_2x > count_1x,
            "2x scale rasterization of 'A' must occupy more pixels ({count_2x}) than 1x ({count_1x})"
        );
    }
}
