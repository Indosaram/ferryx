//! Contract tests for real font rasterization, CJK/Unicode fallback, and cell metrics derivation.

use ferryx_lib::native_terminal::renderer::font_manager::FontManager;

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
    let ascii_mask = global_mgr
        .rasterize_glyph("A", metrics.width_px, metrics.height_px, false, false)
        .into_buffer();
    assert_eq!(
        ascii_mask.len(),
        (metrics.width_px * metrics.height_px * 4) as usize,
        "ASCII mask length must equal width * height * 4"
    );

    // Rasterize CJK char '가' (wide cell: 2 * width)
    let cjk_mask = global_mgr
        .rasterize_glyph("가", metrics.width_px * 2, metrics.height_px, false, false)
        .into_buffer();
    assert_eq!(
        cjk_mask.len(),
        (metrics.width_px * 2 * metrics.height_px * 4) as usize,
        "CJK mask length must equal 2 * width * height * 4"
    );

    let ascii_non_empty = ascii_mask.iter().any(|&b| b > 0);
    assert!(
        ascii_non_empty,
        "ASCII 'A' must produce a non-empty alpha mask when system fonts exist"
    );

    // 2. Missing glyph test: unassigned codepoint must never panic and must return an all-zero alpha mask
    let isolated_mgr = FontManager::new_with_family_and_size("monospace", 14.0);

    let iso_metrics = isolated_mgr.cell_metrics();
    assert!(
        iso_metrics.width_px > 0,
        "fallback cell width without fonts must still be non-zero"
    );
    assert!(
        iso_metrics.height_px > 0,
        "fallback cell height without fonts must still be non-zero"
    );

    // Unassigned PUA codepoint must return an empty mask without panicking
    let isolated_missing = isolated_mgr
        .rasterize_glyph("\u{10fffd}", 10, 20, false, false)
        .into_buffer();
    assert_eq!(isolated_missing.len(), 200);
    assert!(
        isolated_missing.iter().all(|&b| b == 0),
        "unassigned codepoint must return an all-zero alpha mask"
    );

    let whitespace_mask = isolated_mgr
        .rasterize_glyph("   ", 10, 20, false, false)
        .into_buffer();
    assert_eq!(whitespace_mask.len(), 200);
    assert!(
        whitespace_mask.iter().all(|&b| b == 0),
        "whitespace must return an all-zero mask without panic"
    );
}

#[test]
fn test_derived_cell_metrics_consistency_with_configured_font_sizes() {
    for size in [10.0, 13.0, 16.0, 24.0] {
        let mgr = FontManager::new_with_family_and_size("monospace", size);
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

    let mask_1x = global_mgr
        .rasterize_glyph_for_scale("A", m1.width_px, m1.height_px, false, false, 1.0)
        .into_buffer();
    let mask_2x = global_mgr
        .rasterize_glyph_for_scale("A", m2.width_px, m2.height_px, false, false, 2.0)
        .into_buffer();

    assert_eq!(
        mask_1x.len(),
        (m1.width_px * m1.height_px * 4) as usize,
        "1x mask length matches 1x cell dimensions"
    );
    assert_eq!(
        mask_2x.len(),
        (m2.width_px * m2.height_px * 4) as usize,
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

#[test]
fn test_glyph_orientation_regression_contract() {
    let global_mgr = FontManager::global();
    let metrics = global_mgr.cell_metrics();
    let w = metrics.width_px;
    let h = metrics.height_px;
    let mid_y = (h / 2) as usize;

    // 'L': bottom-half ink > top-half ink
    let l_mask = global_mgr
        .rasterize_glyph("L", w, h, false, false)
        .into_buffer();
    let mut top_ink_l = 0u64;
    let mut bottom_ink_l = 0u64;
    for y in 0..h as usize {
        for x in 0..w as usize {
            let idx = (y * (w as usize) + x) * 4;
            let val = l_mask[idx + 3] as u64;
            if y < mid_y {
                top_ink_l += val;
            } else {
                bottom_ink_l += val;
            }
        }
    }
    assert!(
        bottom_ink_l > top_ink_l,
        "'L' bottom-half ink ({bottom_ink_l}) must be strictly greater than top-half ink ({top_ink_l})"
    );

    // 'P': top-half ink > bottom-half ink
    let p_mask = global_mgr
        .rasterize_glyph("P", w, h, false, false)
        .into_buffer();
    let mut top_ink_p = 0u64;
    let mut bottom_ink_p = 0u64;
    for y in 0..h as usize {
        for x in 0..w as usize {
            let idx = (y * (w as usize) + x) * 4;
            let val = p_mask[idx + 3] as u64;
            if y < mid_y {
                top_ink_p += val;
            } else {
                bottom_ink_p += val;
            }
        }
    }
    assert!(
        top_ink_p > bottom_ink_p,
        "'P' top-half ink ({top_ink_p}) must be strictly greater than bottom-half ink ({bottom_ink_p})"
    );

    // '─': horizontal bar must be centered within ±2px of buffer middle
    let line_mask = global_mgr
        .rasterize_glyph("─", w, h, false, false)
        .into_buffer();
    let mut row_sums: Vec<(usize, u64)> = (0..h as usize)
        .map(|y| {
            let sum: u64 = (0..w as usize)
                .map(|x| {
                    let idx = (y * (w as usize) + x) * 4;
                    line_mask[idx + 3] as u64
                })
                .sum();
            (y, sum)
        })
        .collect();
    row_sums.sort_by_key(|&(_, sum)| std::cmp::Reverse(sum));
    let max_ink_row = row_sums[0].0;
    assert!(
        (max_ink_row as isize - mid_y as isize).abs() <= 2,
        "'─' ink row ({max_ink_row}) must be centered within 2px of middle ({mid_y})"
    );
}
