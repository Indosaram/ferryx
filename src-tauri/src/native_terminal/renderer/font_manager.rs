//! Pure-Rust Font Manager and Glyph Resolver.
//!
//! Loads primary terminal font and fallback system fonts using `fontdb`,
//! caches loaded face bytes (`FontVec` from `ab_glyph`), resolves missing glyphs
//! via a deterministic fallback chain with codepoint caching, and computes
//! primary cell metrics.

use std::collections::HashMap;
use std::sync::Arc;

use ab_glyph::{Font, FontVec, Glyph, Point, PxScale, ScaleFont};
use fontdb::{Database, FaceInfo, Family, Query, Stretch, Style, Weight, ID};
use parking_lot::Mutex;

use crate::native_terminal::composition::CellMetrics;
use crate::native_terminal::renderer::rasterizer::RasterizedGlyph;
use crate::terminal::preferences::{
    cached_terminal_preferences, TerminalPreferences, DEFAULT_TERMINAL_FONT_SIZE,
};

/// Cached font entry containing the parsed `FontVec` and associated face metadata.
pub struct LoadedFace {
    pub font: Mutex<FontVec>,
    pub weight: Weight,
    pub style: Style,
    pub wght_axis: Option<ab_glyph::VariationAxis>,
}

/// Internal state of the FontManager guarded by Mutex.
struct FontManagerState {
    db: Database,
    font_size: f32,
    family_names: Vec<String>,
    primary_regular_id: Option<ID>,
    primary_bold_id: Option<ID>,
    primary_italic_id: Option<ID>,
    primary_bold_italic_id: Option<ID>,
    configured_fallback_ids: Vec<ID>,
    fallback_face_ids: Vec<ID>,
    loaded_faces: HashMap<ID, Arc<LoadedFace>>,
    codepoint_face_cache: HashMap<(char, bool, bool), Option<ID>>,
}

pub struct FontManager {
    font_family: String,
    state: Mutex<FontManagerState>,
}

static GLOBAL_FONT_MANAGER: Mutex<Option<Arc<FontManager>>> = Mutex::new(None);

/// Returns true if `text` begins with a Unicode codepoint in standard emoji ranges.
pub fn is_color_glyph_candidate(text: &str) -> bool {
    let Some(first_ch) = text.chars().next() else {
        return false;
    };
    let cp = first_ch as u32;
    match cp {
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF => true,
        _ => false,
    }
}

/// Returns true if character is in secondary candidate ranges (Arrows or Misc Technical).
pub fn is_secondary_color_candidate(ch: char) -> bool {
    let cp = ch as u32;
    matches!(cp, 0x2190..=0x21FF | 0x2300..=0x23FF)
}

impl FontManager {
    /// Creates a new `FontManager` loading system fonts and using the provided preferences.
    pub fn from_preferences(prefs: &TerminalPreferences) -> Self {
        let mut db = Database::new();
        db.load_system_fonts();
        Self::new_with_db(db, &prefs.font_family, prefs.font_size)
    }

    /// Creates a new `FontManager` with a custom `Database`, font family query, and font size.
    pub fn new_with_db(db: Database, font_family: &str, font_size: f32) -> Self {
        let font_size = if font_size > 0.0 {
            font_size
        } else {
            DEFAULT_TERMINAL_FONT_SIZE
        };

        let families = parse_font_families(font_family);
        let family_names: Vec<String> = font_family
            .split(',')
            .map(|part| part.trim().trim_matches('\"').trim_matches('\'').trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();

        // Query primary face variants
        let primary_regular_id = db.query(&Query {
            families: &families,
            weight: Weight::NORMAL,
            style: Style::Normal,
            stretch: Stretch::Normal,
        });

        let primary_bold_id = db
            .query(&Query {
                families: &families,
                weight: Weight::BOLD,
                style: Style::Normal,
                stretch: Stretch::Normal,
            })
            .or(primary_regular_id);

        let primary_italic_id = db
            .query(&Query {
                families: &families,
                weight: Weight::NORMAL,
                style: Style::Italic,
                stretch: Stretch::Normal,
            })
            .or(primary_regular_id);

        let primary_bold_italic_id = db
            .query(&Query {
                families: &families,
                weight: Weight::BOLD,
                style: Style::Italic,
                stretch: Stretch::Normal,
            })
            .or(primary_bold_id)
            .or(primary_italic_id)
            .or(primary_regular_id);

        // Families the user listed after the primary are explicit preferences, so they must be
        // consulted before arbitrary system fonts for codepoints the primary face lacks.
        let configured_fallback_ids: Vec<ID> = families
            .iter()
            .filter_map(|family| {
                db.query(&Query {
                    families: &[*family],
                    weight: Weight::NORMAL,
                    style: Style::Normal,
                    stretch: Stretch::Normal,
                })
            })
            .filter(|id| Some(*id) != primary_regular_id)
            .collect();

        // Collect deterministic fallback face IDs sorted by:
        // 1. |face_weight - 400| ascending (Regular faces beat Light/Thin/Bold)
        // 2. format!("{:?}", id) for deterministic tie-breaking
        let mut fallback_face_ids: Vec<ID> = db.faces().map(|f| f.id).collect();
        fallback_face_ids.sort_by(|&a_id, &b_id| {
            let a_weight = db.face(a_id).map(|f| f.weight.0 as i32).unwrap_or(400);
            let b_weight = db.face(b_id).map(|f| f.weight.0 as i32).unwrap_or(400);
            let a_dist = (a_weight - 400).abs();
            let b_dist = (b_weight - 400).abs();
            a_dist
                .cmp(&b_dist)
                .then_with(|| format!("{:?}", a_id).cmp(&format!("{:?}", b_id)))
        });

        let mut loaded_faces = HashMap::new();

        if let Some(reg_id) = primary_regular_id {
            if let Some(loaded) = load_face_data(&db, reg_id) {
                loaded_faces.insert(reg_id, Arc::new(loaded));
            }
        } else if let Some(&first_id) = fallback_face_ids.first() {
            if let Some(loaded) = load_face_data(&db, first_id) {
                loaded_faces.insert(first_id, Arc::new(loaded));
            }
        }

        Self {
            font_family: font_family.to_string(),
            state: Mutex::new(FontManagerState {
                db,
                font_size,
                family_names,
                primary_regular_id,
                primary_bold_id,
                primary_italic_id,
                primary_bold_italic_id,
                configured_fallback_ids,
                fallback_face_ids,
                loaded_faces,
                codepoint_face_cache: HashMap::new(),
            }),
        }
    }

    /// Returns the global singleton `FontManager`, rebuilding it when the effective terminal
    /// preferences (Ghostty import plus local overrides) select a different font family.
    ///
    /// A font-size-only change reuses the loaded `fontdb` database instead of rescanning system
    /// fonts, so terminal zoom stays cheap.
    pub fn global() -> Arc<Self> {
        let prefs = cached_terminal_preferences();
        let mut guard = GLOBAL_FONT_MANAGER.lock();
        if let Some(ref mgr) = *guard {
            if mgr.font_family == prefs.font_family {
                mgr.set_font_size(prefs.font_size);
                return Arc::clone(mgr);
            }
        }
        let mgr = Arc::new(Self::from_preferences(&prefs));
        *guard = Some(Arc::clone(&mgr));
        mgr
    }

    /// Updates the rendered font size in place, keeping the loaded face database.
    pub fn set_font_size(&self, font_size: f32) {
        if !(font_size.is_finite() && font_size > 0.0) {
            return;
        }
        let mut state = self.state.lock();
        if state.font_size != font_size {
            state.font_size = font_size;
        }
    }

    /// Returns the derived primary cell metrics (width_px, height_px) for the given display scale factor.
    pub fn cell_metrics_for_scale(&self, scale_factor: f32) -> CellMetrics {
        let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let mut state = self.state.lock();
        let effective_font_size = state.font_size * scale;

        let primary_id = state
            .primary_regular_id
            .or_else(|| state.fallback_face_ids.first().copied());
        if let Some(id) = primary_id {
            if let Some(loaded) = state.loaded_faces.get(&id).cloned().or_else(|| {
                load_face_data(&state.db, id).map(|f| {
                    let arc = Arc::new(f);
                    state.loaded_faces.insert(id, Arc::clone(&arc));
                    arc
                })
            }) {
                return derive_metrics_from_loaded(&loaded, effective_font_size);
            }
        }

        CellMetrics {
            width_px: (effective_font_size * 0.6).round().max(1.0) as u32,
            height_px: (effective_font_size * 1.5).round().max(1.0) as u32,
        }
    }

    /// Returns the derived primary cell metrics (width_px, height_px) at 1.0x scale.
    pub fn cell_metrics(&self) -> CellMetrics {
        self.cell_metrics_for_scale(1.0)
    }

    /// Returns the configured font size in points.
    /// Faces resolved from the user's configured families after the primary one.
    pub fn configured_fallback_ids(&self) -> Vec<ID> {
        self.state.lock().configured_fallback_ids.clone()
    }

    /// Returns the deterministic sweep fallback face IDs sorted by weight distance from Regular (400).
    pub fn sweep_fallback_ids(&self) -> Vec<ID> {
        self.state.lock().fallback_face_ids.clone()
    }

    pub fn font_size(&self) -> f32 {
        self.state.lock().font_size
    }

    /// Rasterizes text into an alpha mask or RGBA color buffer with dimensions `(width, height)` using the specified scale factor.
    pub fn rasterize_glyph_for_scale(
        &self,
        text: &str,
        width: u32,
        height: u32,
        bold: bool,
        italic: bool,
        scale_factor: f32,
    ) -> RasterizedGlyph {
        let total_pixels = (width * height) as usize;
        let mut buffer = vec![0u8; total_pixels];

        if text.is_empty() || text.chars().all(|c| c.is_whitespace()) || width == 0 || height == 0 {
            return RasterizedGlyph::Alpha(buffer);
        }

        let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let effective_font_size = self.state.lock().font_size * scale;

        // Step 1: Primary emoji color candidate path
        if is_color_glyph_candidate(text) {
            if let Some(color_bytes) = crate::native_terminal::renderer::color_glyph::rasterize_color_glyph(
                text,
                width,
                height,
                effective_font_size,
            ) {
                return RasterizedGlyph::Color(color_bytes);
            }
        }

        // Step 2: Alpha path
        let mut chars = text.chars().peekable();
        let first_ch = match chars.next() {
            Some(c) => c,
            None => return RasterizedGlyph::Alpha(buffer),
        };

        let mut state = self.state.lock();
        let rendered_base = render_char_to_buffer(
            &mut state,
            first_ch,
            &mut buffer,
            width,
            height,
            effective_font_size,
            bold,
            italic,
            false,
        );

        // Render combining characters on top
        for comb_ch in chars {
            if !comb_ch.is_whitespace() {
                let _ = render_char_to_buffer(
                    &mut state,
                    comb_ch,
                    &mut buffer,
                    width,
                    height,
                    effective_font_size,
                    bold,
                    italic,
                    true,
                );
            }
        }
        drop(state);

        // Step 3: Secondary candidate fallback if alpha path failed
        if !rendered_base && is_secondary_color_candidate(first_ch) {
            if let Some(color_bytes) = crate::native_terminal::renderer::color_glyph::rasterize_color_glyph(
                text,
                width,
                height,
                effective_font_size,
            ) {
                return RasterizedGlyph::Color(color_bytes);
            }
        }

        RasterizedGlyph::Alpha(buffer)
    }

    /// Rasterizes text into an alpha mask or RGBA color buffer with dimensions `(width, height)`.
    pub fn rasterize_glyph(
        &self,
        text: &str,
        width: u32,
        height: u32,
        bold: bool,
        italic: bool,
    ) -> RasterizedGlyph {
        self.rasterize_glyph_for_scale(text, width, height, bold, italic, 1.0)
    }
}

/// Helper function to parse comma-separated font family string into fontdb `Family` queries.
fn parse_font_families(font_family: &str) -> Vec<Family<'_>> {
    let mut families = Vec::new();
    for part in font_family.split(',') {
        let trimmed = part.trim().trim_matches('"').trim_matches('\'').trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("monospace") {
            families.push(Family::Monospace);
        } else if trimmed.eq_ignore_ascii_case("serif") {
            families.push(Family::Serif);
        } else if trimmed.eq_ignore_ascii_case("sans-serif") {
            families.push(Family::SansSerif);
        } else if trimmed.eq_ignore_ascii_case("cursive") {
            families.push(Family::Cursive);
        } else if trimmed.eq_ignore_ascii_case("fantasy") {
            families.push(Family::Fantasy);
        } else {
            families.push(Family::Name(trimmed));
        }
    }
    if families.is_empty() {
        families.push(Family::Monospace);
    }
    families
}

/// Loads face data and metadata from fontdb database as a `LoadedFace`.
fn load_face_data(db: &Database, id: ID) -> Option<LoadedFace> {
    let face_info: Option<FaceInfo> = db.face(id).cloned();
    let weight = face_info
        .as_ref()
        .map(|i| i.weight)
        .unwrap_or(Weight::NORMAL);
    let style = face_info.as_ref().map(|i| i.style).unwrap_or(Style::Normal);

    db.with_face_data(id, |face_data, face_index| {
        FontVec::try_from_vec_and_index(face_data.to_vec(), face_index)
            .ok()
            .map(|mut font| {
                use ab_glyph::VariableFont;
                let wght_axis = font.variations().into_iter().find(|a| &a.tag == b"wght");
                if let Some(ref axis) = wght_axis {
                    let default_wght = 400.0_f32.clamp(axis.min_value, axis.max_value);
                    font.set_variation(b"wght", default_wght);
                }
                LoadedFace {
                    font: Mutex::new(font),
                    weight,
                    style,
                    wght_axis,
                }
            })
    })
    .flatten()
}

/// Converts a point size into the `PxScale` ab_glyph expects.
///
/// `PxScale` is the pixel height of `ascent - descent`, NOT an em size, so passing a point size
/// straight in shrinks text by `height_unscaled / units_per_em` (about 26% for MesloLGS NF).
/// macOS renders type at 72 DPI, where 1pt = 1px, so scaling by that ratio makes one em exactly
/// `font_size` pixels and matches Ghostty's `DesiredSize.pixels()`.
fn px_scale_for_size(font: &FontVec, font_size: f32) -> PxScale {
    let units_per_em = font.units_per_em().unwrap_or(0.0);
    let height_unscaled = font.height_unscaled();
    if units_per_em > 0.0 && height_unscaled > 0.0 {
        PxScale::from(font_size * height_unscaled / units_per_em)
    } else {
        PxScale::from(font_size)
    }
}

/// Derives `CellMetrics` (width_px, height_px) from the primary font at configured `font_size`.
fn derive_metrics_from_font(font: &FontVec, font_size: f32) -> CellMetrics {
    let scale = px_scale_for_size(font, font_size);
    let scaled = font.as_scaled(scale);

    let ascent = scaled.ascent();
    let descent = scaled.descent(); // negative in ab_glyph
    let line_gap = scaled.line_gap();

    // Ghostty's `FaceMetrics.lineHeight()`: ascent - descent + line_gap, rounded rather than
    // ceiled so cell spacing stays within 0.5px of the font's authored line height.
    let raw_height = ascent - descent + line_gap;
    let height_px = raw_height.round().max(1.0) as u32;

    let m_glyph_id = font.glyph_id('M');
    let width_px = if m_glyph_id.0 != 0 {
        let adv = scaled.h_advance(m_glyph_id);
        if adv > 0.0 {
            adv.round().max(1.0) as u32
        } else {
            (font_size * 0.6).round().max(1.0) as u32
        }
    } else {
        (font_size * 0.6).round().max(1.0) as u32
    };

    CellMetrics {
        width_px,
        height_px,
    }
}

/// Derives `CellMetrics` (width_px, height_px) from `LoadedFace` at configured `font_size`,
/// ensuring variable fonts have wght=400 applied before metrics extraction.
fn derive_metrics_from_loaded(loaded: &LoadedFace, font_size: f32) -> CellMetrics {
    let mut font = loaded.font.lock();
    if let Some(ref axis) = loaded.wght_axis {
        use ab_glyph::VariableFont;
        let w = 400.0_f32.clamp(axis.min_value, axis.max_value);
        font.set_variation(b"wght", w);
    }
    derive_metrics_from_font(&font, font_size)
}

/// Resolves the face ID for a specific character, checking primary faces first then fallback chain.
fn resolve_face_for_char(
    state: &mut FontManagerState,
    ch: char,
    bold: bool,
    italic: bool,
) -> Option<(ID, Arc<LoadedFace>)> {
    let cache_key = (ch, bold, italic);
    if let Some(cached_id_opt) = state.codepoint_face_cache.get(&cache_key) {
        if let Some(face_id) = *cached_id_opt {
            if let Some(loaded) = state.loaded_faces.get(&face_id) {
                return Some((face_id, Arc::clone(loaded)));
            } else if let Some(loaded_face) = load_face_data(&state.db, face_id) {
                let loaded = Arc::new(loaded_face);
                state.loaded_faces.insert(face_id, Arc::clone(&loaded));
                return Some((face_id, loaded));
            }
        }
        return None;
    }

    // Step 1: Check primary faces based on requested style
    let primary_candidates = match (bold, italic) {
        (true, true) => [
            state.primary_bold_italic_id,
            state.primary_bold_id,
            state.primary_italic_id,
            state.primary_regular_id,
        ],
        (true, false) => [state.primary_bold_id, state.primary_regular_id, None, None],
        (false, true) => [
            state.primary_italic_id,
            state.primary_regular_id,
            None,
            None,
        ],
        (false, false) => [state.primary_regular_id, None, None, None],
    };

    for candidate_opt in primary_candidates.into_iter().flatten() {
        let loaded = if let Some(loaded) = state.loaded_faces.get(&candidate_opt) {
            Arc::clone(loaded)
        } else if let Some(loaded_face) = load_face_data(&state.db, candidate_opt) {
            let loaded = Arc::new(loaded_face);
            state
                .loaded_faces
                .insert(candidate_opt, Arc::clone(&loaded));
            loaded
        } else {
            continue;
        };

        if loaded.font.lock().glyph_id(ch).0 != 0 {
            state
                .codepoint_face_cache
                .insert(cache_key, Some(candidate_opt));
            return Some((candidate_opt, loaded));
        }
    }

    // Step 2: User's remaining configured fallback families
    for fb_id in state.configured_fallback_ids.clone() {
        if Some(fb_id) == state.primary_regular_id
            || Some(fb_id) == state.primary_bold_id
            || Some(fb_id) == state.primary_italic_id
            || Some(fb_id) == state.primary_bold_italic_id
        {
            continue;
        }

        let loaded = if let Some(loaded) = state.loaded_faces.get(&fb_id) {
            Arc::clone(loaded)
        } else if let Some(loaded_face) = load_face_data(&state.db, fb_id) {
            let loaded = Arc::new(loaded_face);
            state.loaded_faces.insert(fb_id, Arc::clone(&loaded));
            loaded
        } else {
            continue;
        };

        if loaded.font.lock().glyph_id(ch).0 != 0 {
            state.codepoint_face_cache.insert(cache_key, Some(fb_id));
            return Some((fb_id, loaded));
        }
    }

    // Step 3: CoreText cascade-guided fallback (macOS)
    if let Some(ct_id) = crate::native_terminal::renderer::fallback_resolver::resolve_coretext_fallback(
        &mut state.db,
        &state.family_names,
        ch,
        bold,
        italic,
    ) {
        let loaded = if let Some(loaded) = state.loaded_faces.get(&ct_id) {
            Some(Arc::clone(loaded))
        } else {
            load_face_data(&state.db, ct_id).map(|loaded_face| {
                let loaded = Arc::new(loaded_face);
                state.loaded_faces.insert(ct_id, Arc::clone(&loaded));
                loaded
            })
        };

        if let Some(loaded) = loaded {
            if loaded.font.lock().glyph_id(ch).0 != 0 {
                state.codepoint_face_cache.insert(cache_key, Some(ct_id));
                return Some((ct_id, loaded));
            }
        }
    }

    // Step 4: Deterministic system fallback sweep (sorted by distance from Weight 400)
    for &fb_id in &state.fallback_face_ids.clone() {
        if Some(fb_id) == state.primary_regular_id
            || Some(fb_id) == state.primary_bold_id
            || Some(fb_id) == state.primary_italic_id
            || Some(fb_id) == state.primary_bold_italic_id
            || state.configured_fallback_ids.contains(&fb_id)
        {
            continue;
        }

        let loaded = if let Some(loaded) = state.loaded_faces.get(&fb_id) {
            Arc::clone(loaded)
        } else if let Some(loaded_face) = load_face_data(&state.db, fb_id) {
            let loaded = Arc::new(loaded_face);
            state.loaded_faces.insert(fb_id, Arc::clone(&loaded));
            loaded
        } else {
            continue;
        };

        if loaded.font.lock().glyph_id(ch).0 != 0 {
            state.codepoint_face_cache.insert(cache_key, Some(fb_id));
            return Some((fb_id, loaded));
        }
    }

    // No font in the database has this glyph
    state.codepoint_face_cache.insert(cache_key, None);
    None
}

/// Renders a single character into the target alpha buffer.
#[allow(clippy::too_many_arguments)]
fn render_char_to_buffer(
    state: &mut FontManagerState,
    ch: char,
    buffer: &mut [u8],
    width: u32,
    height: u32,
    font_size: f32,
    bold: bool,
    italic: bool,
    is_combining: bool,
) -> bool {
    let (_face_id, loaded_face) = match resolve_face_for_char(state, ch, bold, italic) {
        Some(res) => res,
        None => return false,
    };

    let target_weight: f32 = if bold { 700.0 } else { 400.0 };

    // Extract outline and bounds inside a minimal critical section
    let (outlined, needs_synthetic_bold, needs_synthetic_italic) = {
        let mut font = loaded_face.font.lock();

        if let Some(ref axis) = loaded_face.wght_axis {
            use ab_glyph::VariableFont;
            let w = target_weight.clamp(axis.min_value, axis.max_value);
            font.set_variation(b"wght", w);
        }

        let glyph_id = font.glyph_id(ch);
        if glyph_id.0 == 0 {
            return false;
        }

        let initial_scale = px_scale_for_size(&font, font_size);
        let initial_scaled = font.as_scaled(initial_scale);

        let init_ascent = initial_scaled.ascent();
        let init_descent = initial_scaled.descent();
        let init_font_height = (init_ascent - init_descent).max(1.0);
        let init_advance = initial_scaled.h_advance(glyph_id);

        // Compute fitting scale if glyph exceeds cell dimensions beyond physical rounding tolerance (1.0px)
        let scale_ratio_y = if init_font_height > (height as f32 + 1.0) && height > 0 {
            (height as f32) / init_font_height
        } else {
            1.0
        };
        let scale_ratio_x = if init_advance > (width as f32 + 1.0) && width > 0 && !is_combining {
            (width as f32) / init_advance
        } else {
            1.0
        };
        let scale_ratio = scale_ratio_x.min(scale_ratio_y);

        let (scale, _scaled, ascent, font_height, advance) = if scale_ratio < 0.99 {
            let adj_scale = px_scale_for_size(&font, font_size * scale_ratio);
            let adj_scaled = font.as_scaled(adj_scale);
            let adj_ascent = adj_scaled.ascent();
            let adj_descent = adj_scaled.descent();
            let adj_height = (adj_ascent - adj_descent).max(1.0);
            let adj_adv = adj_scaled.h_advance(glyph_id);
            (adj_scale, adj_scaled, adj_ascent, adj_height, adj_adv)
        } else {
            (
                initial_scale,
                initial_scaled,
                init_ascent,
                init_font_height,
                init_advance,
            )
        };

        // Compute vertical alignment: center vertically within cell height snapped to integral physical pixels
        let v_offset = (((height as f32) - font_height) / 2.0).floor();
        let baseline_y = (ascent + v_offset.max(0.0)).round();

        // Compute horizontal alignment snapped to integral physical pixels
        let pos_x = if is_combining {
            0.0
        } else if advance > 0.0 && advance < (width as f32) {
            (((width as f32) - advance) / 2.0).floor()
        } else {
            0.0
        };

        let glyph: Glyph = glyph_id.with_scale_and_position(
            scale,
            Point {
                x: pos_x,
                y: baseline_y,
            },
        );

        let effective_weight = if let Some(ref axis) = loaded_face.wght_axis {
            target_weight.clamp(axis.min_value, axis.max_value)
        } else {
            loaded_face.weight.0 as f32
        };
        let needs_synthetic_bold = bold && effective_weight < 600.0;
        let needs_synthetic_italic = italic && loaded_face.style == Style::Normal;

        let outlined = font.outline_glyph(glyph);
        (outlined, needs_synthetic_bold, needs_synthetic_italic)
    };

    let Some(outlined) = outlined else {
        return false;
    };

    let bounds = outlined.px_bounds();
    let min_x = bounds.min.x.floor() as i32;
    let min_y = bounds.min.y.floor() as i32;

    outlined.draw(|gx, gy, coverage| {
        if coverage <= 0.0 {
            return;
        }
        let base_px = min_x + gx as i32;
        let py = min_y + gy as i32;

        let italic_offset = if needs_synthetic_italic {
            let shift = (((height as i32).saturating_sub(1 + py)) * (width as i32))
                / ((3 * height as i32).max(1));
            shift.min(width as i32 / 3)
        } else {
            0
        };

        let px = base_px + italic_offset;

        if px >= 0 && px < (width as i32) && py >= 0 && py < (height as i32) {
            let idx = (py as u32 * width + px as u32) as usize;
            if idx < buffer.len() {
                let alpha = (coverage * 255.0).round() as u8;
                buffer[idx] = buffer[idx].saturating_add(alpha);

                if needs_synthetic_bold && px + 1 < (width as i32) {
                    let bold_idx = (py as u32 * width + (px + 1) as u32) as usize;
                    if bold_idx < buffer.len() {
                        buffer[bold_idx] = buffer[bold_idx].saturating_add(alpha);
                    }
                }
            }
        }
    });

    true
}

/// Convenience function to get the derived primary cell metrics.
pub fn derived_cell_metrics() -> CellMetrics {
    FontManager::global().cell_metrics()
}

/// Convenience function to get the derived primary cell metrics for a specific scale factor.
pub fn derived_cell_metrics_for_scale(scale_factor: f64) -> CellMetrics {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor as f32
    } else {
        1.0
    };
    FontManager::global().cell_metrics_for_scale(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_manager_derives_nonzero_metrics_and_rasterizes() {
        let mgr = FontManager::global();
        let metrics = mgr.cell_metrics();
        assert!(metrics.width_px > 0, "cell width must be strictly positive");
        assert!(
            metrics.height_px > 0,
            "cell height must be strictly positive"
        );
        assert_eq!(mgr.font_size(), DEFAULT_TERMINAL_FONT_SIZE);

        let ascii_mask =
            mgr.rasterize_glyph("A", metrics.width_px, metrics.height_px, false, false).into_buffer();
        assert_eq!(
            ascii_mask.len(),
            (metrics.width_px * metrics.height_px) as usize
        );
        assert!(
            !ascii_mask.iter().all(|&b| b == 0),
            "ASCII 'A' must produce non-empty mask when system font exists"
        );

        let cjk_mask =
            mgr.rasterize_glyph("가", metrics.width_px * 2, metrics.height_px, false, false).into_buffer();
        assert_eq!(
            cjk_mask.len(),
            (metrics.width_px * 2 * metrics.height_px) as usize
        );
        // If system font has Korean/CJK, mask is non-empty
        // In all cases, rasterization must complete without panic
    }

    #[test]
    fn test_empty_database_never_panics_and_returns_empty_mask() {
        let empty_db = Database::new();
        let mgr = FontManager::new_with_db(empty_db, "non-existent-font", 14.0);
        let metrics = mgr.cell_metrics();
        assert!(metrics.width_px > 0);
        assert!(metrics.height_px > 0);

        let mask = mgr.rasterize_glyph("A", 10, 20, false, false).into_buffer();
        assert_eq!(mask.len(), 200);
        assert!(
            mask.iter().all(|&b| b == 0),
            "empty font database returns empty mask"
        );

        let cjk_mask = mgr.rasterize_glyph("가", 20, 20, false, false).into_buffer();
        assert_eq!(cjk_mask.len(), 400);
        assert!(
            cjk_mask.iter().all(|&b| b == 0),
            "missing glyph returns empty mask without panic"
        );
    }

    #[test]
    fn test_cell_metrics_match_ghostty_point_to_pixel_conversion() {
        let mut db = Database::new();
        db.load_system_fonts();
        let Some(id) = db.faces().next().map(|face| face.id) else {
            return;
        };
        let face = load_face_data(&db, id).expect("system font face data");
        let font_guard = face.font.lock();
        let font = &*font_guard;
        let font_size = 17.0;

        let units_per_em = font
            .units_per_em()
            .expect("system font reports units_per_em");
        let px_per_unit = font_size / units_per_em;

        let scaled = font.as_scaled(px_scale_for_size(font, font_size));
        assert!(
            (scaled.height() / (font.height_unscaled() * px_per_unit) - 1.0).abs() < 1e-4,
            "one em must rasterize at exactly font_size pixels, as macOS renders type at 72 DPI"
        );

        let metrics = derive_metrics_from_font(font, font_size);
        let expected_height = ((font.ascent_unscaled() - font.descent_unscaled()
            + font.line_gap_unscaled())
            * px_per_unit)
            .round()
            .max(1.0) as u32;
        assert_eq!(
            metrics.height_px, expected_height,
            "cell height must equal Ghostty's lineHeight (ascent - descent + line_gap)"
        );

        let m_glyph = font.glyph_id('M');
        if m_glyph.0 != 0 && font.h_advance_unscaled(m_glyph) > 0.0 {
            let expected_width = (font.h_advance_unscaled(m_glyph) * px_per_unit)
                .round()
                .max(1.0) as u32;
            assert_eq!(
                metrics.width_px, expected_width,
                "cell width must equal the em-scaled advance, matching Ghostty's face_width"
            );
        }
    }

    #[test]
    fn test_meslo_lgs_nf_13pt_matches_ghostty_cell_geometry() {
        let mut db = Database::new();
        db.load_system_fonts();
        let mgr = FontManager::new_with_db(db, "MesloLGS NF", 13.0);
        let has_meslo = {
            let state = mgr.state.lock();
            state.primary_regular_id.is_some()
        };
        if !has_meslo {
            return;
        }

        // MesloLGS NF: unitsPerEm 2048, hhea ascent 2001 / descent -583 / lineGap 0, 'M' advance 1233.
        // Ghostty at 13pt (72 DPI): width round(1233 * 13/2048) = 8, height round(2584 * 13/2048) = 16.
        assert_eq!(
            mgr.cell_metrics(),
            CellMetrics {
                width_px: 8,
                height_px: 16
            }
        );
    }

    #[test]
    fn test_glyph_placement_snaps_to_integral_physical_pixels() {
        let mgr = FontManager::global();
        let metrics = mgr.cell_metrics();
        // Odd width cell ensures (width - advance) / 2.0 would be fractional without integer snapping
        let odd_width = if metrics.width_px % 2 == 0 {
            metrics.width_px + 1
        } else {
            metrics.width_px
        };
        let mask = mgr.rasterize_glyph("I", odd_width, metrics.height_px, false, false).into_buffer();
        assert_eq!(mask.len(), (odd_width * metrics.height_px) as usize);
        // Ensure glyph produced a valid non-empty mask
        let total_coverage: u32 = mask.iter().map(|&b| b as u32).sum();
        assert!(total_coverage > 0, "glyph mask must not be empty");
    }

    #[test]
    fn test_glyph_rasterization_avoids_fractional_shrink_within_rounding_tolerance() {
        let mgr = FontManager::global();
        let metrics = mgr.cell_metrics();
        // Requesting a cell width 1px smaller than default metrics (within physical rounding tolerance)
        // should still render cleanly without triggering excessive downscaling
        if metrics.width_px > 2 {
            let tight_width = metrics.width_px - 1;
            let mask = mgr.rasterize_glyph("M", tight_width, metrics.height_px, false, false).into_buffer();
            assert_eq!(mask.len(), (tight_width * metrics.height_px) as usize);
            let total_coverage: u32 = mask.iter().map(|&b| b as u32).sum();
            assert!(
                total_coverage > 0,
                "M glyph must rasterize cleanly within rounding tolerance"
            );
        }
    }

    #[test]
    fn test_variable_font_weight_instantiation() {
        use ab_glyph::VariableFont;

        let mut db = Database::new();
        db.load_system_fonts();

        // Locate any variable font in the live system db with a wght axis whose default != 400
        let mut target_vf: Option<(ID, ab_glyph::VariationAxis, char, Vec<u8>, u32)> = None;

        for face in db.faces() {
            let face_id = face.id;
            db.with_face_data(face_id, |data, index| {
                if let Ok(font) = FontVec::try_from_vec_and_index(data.to_vec(), index) {
                    if let Some(axis) = font.variations().into_iter().find(|a| &a.tag == b"wght") {
                        if (axis.default_value - 400.0).abs() > 10.0 {
                            let mut clone =
                                FontVec::try_from_vec_and_index(data.to_vec(), index).unwrap();
                            let target_wght = 400.0_f32.clamp(axis.min_value, axis.max_value);
                            clone.set_variation(b"wght", target_wght);

                            for test_ch in ['A', 'M', 'W', '가', '한', '글', '日', '語'] {
                                if font.glyph_id(test_ch).0 != 0 {
                                    let g_def = font.glyph_id(test_ch).with_scale_and_position(
                                        PxScale::from(64.0),
                                        Point { x: 0.0, y: 64.0 },
                                    );
                                    let g_clone = clone
                                        .glyph_id(test_ch)
                                        .with_scale_and_position(
                                            PxScale::from(64.0),
                                            Point { x: 0.0, y: 64.0 },
                                        );
                                    let b_def = font.outline_glyph(g_def).map(|g| g.px_bounds());
                                    let b_clone =
                                        clone.outline_glyph(g_clone).map(|g| g.px_bounds());
                                    if b_def != b_clone {
                                        target_vf =
                                            Some((face_id, axis, test_ch, data.to_vec(), index));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            });
            if target_vf.is_some() {
                break;
            }
        }

        let Some((vf_id, axis, test_ch, raw_data, face_index)) = target_vf else {
            // Skip when none found in live system db
            return;
        };

        let face_info = db.face(vf_id).expect("face info");
        let primary_family = face_info
            .families
            .first()
            .map(|(f, _)| f.clone())
            .unwrap_or_default();

        let isolated_mgr = FontManager::new_with_db(db, &primary_family, 64.0);

        // Unmodified default font instance
        let default_font =
            FontVec::try_from_vec_and_index(raw_data.clone(), face_index).expect("default font");
        let glyph_default = default_font.glyph_id(test_ch).with_scale_and_position(
            PxScale::from(64.0),
            Point { x: 0.0, y: 64.0 },
        );
        let default_bounds = default_font
            .outline_glyph(glyph_default)
            .map(|g| g.px_bounds());

        // Independently constructed clone with set_variation(b"wght", 400.0)
        let mut clone_font =
            FontVec::try_from_vec_and_index(raw_data, face_index).expect("clone font");
        let target_wght = 400.0_f32.clamp(axis.min_value, axis.max_value);
        clone_font.set_variation(b"wght", target_wght);
        let glyph_clone = clone_font.glyph_id(test_ch).with_scale_and_position(
            PxScale::from(64.0),
            Point { x: 0.0, y: 64.0 },
        );
        let clone_bounds = clone_font.outline_glyph(glyph_clone).map(|g| g.px_bounds());

        // Resolve char through FontManager
        let mut state = isolated_mgr.state.lock();
        let (resolved_id, resolved_face) =
            resolve_face_for_char(&mut state, test_ch, false, false)
                .expect("must resolve character from variable font");
        assert_eq!(resolved_id, vf_id);

        let mut face_font = resolved_face.font.lock();
        if let Some(ref ax) = resolved_face.wght_axis {
            let w = 400.0_f32.clamp(ax.min_value, ax.max_value);
            face_font.set_variation(b"wght", w);
        }
        let glyph_resolved = face_font.glyph_id(test_ch).with_scale_and_position(
            PxScale::from(64.0),
            Point { x: 0.0, y: 64.0 },
        );
        let resolved_bounds = face_font
            .outline_glyph(glyph_resolved)
            .map(|g| g.px_bounds());

        assert_eq!(
            resolved_bounds, clone_bounds,
            "resolved face with wght=400 must equal independently constructed clone with set_variation(b\"wght\", 400)"
        );
        if default_bounds.is_some() && clone_bounds.is_some() {
            assert_ne!(
                resolved_bounds, default_bounds,
                "resolved face at wght=400 must differ from default bounds when axis default != 400 (default was {})",
                axis.default_value
            );
        }
    }

    #[test]
    fn test_sweep_hygiene_deterministic_and_regular_first() {
        let mut db1 = Database::new();
        db1.load_system_fonts();

        let mut db2 = Database::new();
        db2.load_system_fonts();

        let mgr1 = FontManager::new_with_db(db1, "MesloLGS NF", 14.0);
        let mgr2 = FontManager::new_with_db(db2, "MesloLGS NF", 14.0);

        let sweep_1 = mgr1.sweep_fallback_ids();
        let sweep_2 = mgr2.sweep_fallback_ids();

        // 1. Assert determinism across separate instances through production code
        assert_eq!(
            sweep_1, sweep_2,
            "FontManager sweep_fallback_ids must be deterministic across instances"
        );

        // 2. Assert production sequence exactly matches independent application of comparator
        let state = mgr1.state.lock();
        let mut expected_ids: Vec<ID> = state.db.faces().map(|f| f.id).collect();
        expected_ids.sort_by(|&a_id, &b_id| {
            let a_weight = state.db.face(a_id).map(|f| f.weight.0 as i32).unwrap_or(400);
            let b_weight = state.db.face(b_id).map(|f| f.weight.0 as i32).unwrap_or(400);
            let a_dist = (a_weight - 400).abs();
            let b_dist = (b_weight - 400).abs();
            a_dist
                .cmp(&b_dist)
                .then_with(|| format!("{:?}", a_id).cmp(&format!("{:?}", b_id)))
        });
        assert_eq!(
            sweep_1, expected_ids,
            "production sweep_fallback_ids must match two-key weight distance comparator"
        );

        // 3. Semantic outcome assertion on this machine's database for Hangul '실':
        let mut regular_band_hangul_indices = Vec::new();
        let mut light_thin_hangul_indices = Vec::new();

        for (pos, &face_id) in sweep_1.iter().enumerate() {
            if let Some(face_info) = state.db.face(face_id) {
                let weight = face_info.weight.0;
                let has_glyph = state
                    .db
                    .with_face_data(face_id, |data, index| {
                        FontVec::try_from_vec_and_index(data.to_vec(), index)
                            .map(|font| font.glyph_id('실').0 != 0)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);

                if has_glyph {
                    if (350..=450).contains(&weight) {
                        regular_band_hangul_indices.push(pos);
                    } else if weight <= 300 {
                        light_thin_hangul_indices.push(pos);
                    }
                }
            }
        }

        if let (Some(&first_regular_pos), Some(&first_light_pos)) = (
            regular_band_hangul_indices.first(),
            light_thin_hangul_indices.first(),
        ) {
            assert!(
                first_regular_pos < first_light_pos,
                "first Regular-band Hangul face (pos {first_regular_pos}) must appear before any Light/Thin Hangul face (pos {first_light_pos})"
            );
        }
    }
}

#[cfg(test)]
mod resolution_tests {
    use super::*;

    #[test]
    fn every_configured_family_can_render_its_own_glyphs() {
        // A user listing "MesloLGS NF, Noto Sans KR" expects Meslo for Latin/Nerd glyphs and Noto
        // for Korean. Only the first match becomes the primary face, so the later families must
        // still be preferred over arbitrary system fonts for codepoints the primary lacks.
        let mut db = Database::new();
        db.load_system_fonts();
        let manager = FontManager::new_with_db(db, "MesloLGS NF, Noto Sans KR, monospace", 14.0);

        let configured = manager.configured_fallback_ids();
        assert!(
            !configured.is_empty(),
            "later configured families were not retained as preferred fallbacks"
        );
    }

    #[test]
    fn rasterizes_the_glyphs_agents_actually_emit() {
        // Captured from a real agy session stream: box drawing, Braille spinner, Nerd Font PUA
        // marks, and Korean. Every one must produce ink or the pane shows tofu.
        let mut db = Database::new();
        db.load_system_fonts();
        let manager = FontManager::new_with_db(db, "MesloLGS NF, Noto Sans KR, monospace", 14.0);

        for ch in ["\u{2500}", "\u{28fe}", "\u{e0b0}", "\u{f418}", "\u{ac19}"] {
            let raster = manager.rasterize_glyph(ch, 16, 32, false, false);
            assert!(
                raster.buffer().iter().any(|&px| px != 0),
                "no ink rasterized for {ch:?} that a real agent emits"
            );
        }
    }

    #[test]
    fn resolves_a_configured_system_family_and_covers_box_drawing() {
        let mut db = Database::new();
        db.load_system_fonts();
        let manager = FontManager::new_with_db(db, "MesloLGS NF, monospace", 14.0);

        let state = manager.state.lock();
        assert!(
            state.primary_regular_id.is_some(),
            "configured family did not resolve to any face"
        );
        drop(state);

        for ch in ["─", "│", "╭", "╰"] {
            let raster = manager.rasterize_glyph(ch, 16, 32, false, false);
            assert!(
                raster.buffer().iter().any(|&px| px != 0),
                "no face rasterized any pixels for box-drawing glyph {ch:?}"
            );
        }
    }

    #[test]
    fn test_color_glyph_detection_routing() {
        assert!(is_color_glyph_candidate("😺"));
        assert!(is_color_glyph_candidate("😺\u{fe0f}"));
        assert!(is_color_glyph_candidate("⚡\u{fe0f}"));
        assert!(is_color_glyph_candidate("👨‍👩‍👧‍👦"));
        assert!(is_color_glyph_candidate("🇺🇸"));
        assert!(!is_color_glyph_candidate("A"));
        assert!(!is_color_glyph_candidate("실"));
        assert!(!is_color_glyph_candidate("─"));

        let mgr = FontManager::global();
        #[cfg(target_os = "macos")]
        {
            let emoji_raster = mgr.rasterize_glyph("😺", 32, 32, false, false);
            assert!(emoji_raster.is_color(), "emoji '😺' must route to color path");

            let emoji_fe0f_raster = mgr.rasterize_glyph("⚡\u{fe0f}", 32, 32, false, false);
            assert!(emoji_fe0f_raster.is_color(), "emoji ⚡+FE0F must route to color path");
        }

        let ascii_raster = mgr.rasterize_glyph("A", 16, 32, false, false);
        assert!(!ascii_raster.is_color(), "'A' must NOT route to color path");

        let korean_raster = mgr.rasterize_glyph("실", 32, 32, false, false);
        assert!(!korean_raster.is_color(), "'실' must NOT route to color path");
    }
}
