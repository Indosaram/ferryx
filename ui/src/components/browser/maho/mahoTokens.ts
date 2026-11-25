// Maho sidebar design tokens, copied from the Maho Browser sources:
//   maho-chromium/browser/ui/views/sidebar/maho_sidebar_layout_tokens.h
//   maho-chromium/browser/ui/theme/maho_design_tokens.h
//   maho-chromium/browser/ui/views/sidebar/maho_sidebar_tab_list_view.cc
//   maho-chromium/browser/ui/views/sidebar/maho_sidebar_footer_view.cc
//   maho-chromium/browser/ui/views/sidebar/maho_sidebar_library_rail_view.cc
//   maho-chromium/browser/ui/views/sidebar/maho_sidebar_favorites_grid_view.cc
//   maho-chromium/browser/ui/views/sidebar/maho_sidebar_space_dot_view.h
// Every number below is a pixel value at 1x. Maho's dp and CSS px match at the
// sidebar's scale, so the port keeps the raw numbers instead of rescaling.
//
// This module is the single source of truth for Maho geometry. `maho.css` repeats
// the same numbers as CSS custom-property fallbacks; `mahoSpacePalette.test.ts`
// pins the two against each other so they cannot drift.
//
// Scope note: nothing here overrides a Ferryx global theme token. These are
// browser-mode-local measurements, not colors.

import "./maho.css";

/** Maho `maho::tokens` spacing ramp (4px base), `_core-tokens.css` `--space-N`. */
export const MAHO_SPACING_SCALE = {
  0: 0,
  1: 4,
  2: 8,
  3: 12,
  4: 16,
  5: 20,
  6: 24,
  7: 32,
  8: 40,
  9: 48,
  10: 64,
} as const;

/** Maho `maho::tokens` radius ramp: kRadiusSm/Md/Lg/Xl/Pill. */
export const MAHO_RADIUS_SCALE = {
  sm: 8,
  md: 12,
  lg: 16,
  xl: 22,
  pill: 9999,
} as const;

/** Maho `maho::tokens` motion durations: kDurationFast/Medium/SlowMs. */
export const MAHO_MOTION = {
  durationFastMs: 150,
  durationMediumMs: 200,
  durationSlowMs: 300,
  /** kSidebarRevealDurationMs (layout_tokens.h). */
  sidebarRevealMs: 250,
  /** kSidebarHideDelayMs (layout_tokens.h). */
  sidebarHideDelayMs: 300,
} as const;

export const MAHO_LAYOUT_TOKENS = {
  /** kCollapsedRailWidthDp / kDefaultRailWidthDp / min / max / kCompactNavigationThresholdDp. */
  rail: {
    collapsedWidth: 0,
    defaultWidth: 288,
    minWidth: 280,
    maxWidth: 1600,
    compactNavigationThreshold: 208,
    cornerRadius: 18,
    sectionCornerRadius: 12,
    sectionSpacing: 7,
    topRegionSpacing: 8,
    contentInset: 4,
    insets: { top: 6, left: 8, bottom: 8, right: 8 },
    sectionInsets: { top: 2, bottom: 2 },
    rowInsets: { top: 2, left: 7, bottom: 2, right: 7 },
  },
  /**
   * Top bar row: kTopBarTrafficLightAlignmentHeightDp (20) plus kTopBarInsets
   * TLBR(0, 1, 1, 1). Maho aligns the row against the live NSWindow traffic-light
   * center at runtime (maho_traffic_light_geometry.h); 21 is the resting height
   * the port needs for a DOM row, not a substitute for that measurement.
   */
  topBar: {
    height: 21,
    trafficLightAlignmentWidth: 64,
    trafficLightAlignmentHeight: 20,
    buttonSize: 20,
    buttonCornerRadius: 8,
    iconSize: 16,
    buttonSpacing: 0,
    compactLeadingGap: 2,
    compactNavSpacing: 4,
    compactTrailingInset: 3,
    insets: { top: 0, left: 1, bottom: 1, right: 1 },
  },
  /** kControlIndicator* dp values. */
  controlIndicator: {
    height: 32,
    compactWidth: 76,
    cornerRadius: 10,
    iconSize: 16,
    horizontalInset: 8,
    verticalInset: 4,
    itemSpacing: 6,
    minTextColumn: 64,
  },
  /** kSearchPill* dp values. */
  searchPill: {
    height: 35,
    cornerRadius: 10,
    compactHeight: 32,
    compactCornerRadius: 12,
    iconSize: 16,
    insets: { top: 0, left: 2, bottom: 0, right: 2 },
  },
  /** kLibraryRail* / kLibrary* dp values, including the gradient span contract. */
  library: {
    railWidth: 92,
    overlayContentWidth: 380,
    /** kThemedBackgroundGradientSpanDp == kLibraryOverlayContentWidthDp. */
    gradientSpan: 380,
    contentSeparatorThickness: 1,
    railInsets: { top: 14, left: 0, bottom: 14, right: 0 },
    utilitySpacing: 8,
    utilityButtonSize: 24,
    utilityButtonCornerRadius: 10,
    itemWidth: 80,
    itemContentWidth: 68,
    itemCornerRadius: 12,
    itemSpacing: 4,
    itemVerticalInset: 8,
    itemHorizontalInset: 6,
    itemStackVisualLift: 28,
    iconSize: 24,
    iconLabelSpacing: 4,
    rightMargin: 8,
    emptyStateIconSize: 28,
    emptyStateMaxWidth: 252,
    emptyStateItemSpacing: 8,
  },
  /** kMahoSidebarFavoriteSlotCount / kFavoritesColumns / kFavoriteTile* . */
  favorites: {
    capacity: 12,
    columns: 4,
    tileHeight: 47,
    tileCornerRadius: 12,
    tileSpacing: 6,
    tileInsets: { top: 6, left: 4, bottom: 6, right: 4 },
    glyphContainerSize: 28,
    glyphContainerRadius: 10,
    glyphSize: 16,
    labelMaxWidth: 60,
    sectionSpacing: 6,
    emptyStateHeight: 108,
  },
  /** MahoSidebarTabListView kRowHeightDp / kFolderRowHeightDp / kActionRowHeightDp. */
  rows: {
    height: 36,
    cornerRadius: 6,
    iconSize: 17,
    iconTextSpacing: 12,
    closeButtonSize: 24,
    closeIconSize: 15,
    folderHeight: 36,
    folderGlyphSize: 14,
    folderGlyphContainerSize: 18,
    folderLeadingInset: 8,
    sectionSeparatorHeight: 14,
    sectionSeparatorLineHeight: 1,
    inlineActionIconSize: 17,
    inlineActionsSpacing: 6,
    /** `CreateActionRow()`'s kActionRowHeightDp. */
    actionRowHeight: 42,
    actionRowInsets: { top: 0, left: 10, bottom: 0, right: 8 },
  },
  /** MahoSidebarFooterView constants. */
  footer: {
    rowHeight: 32,
    controlSize: 28,
    controlCornerRadius: 14,
    buttonIconSize: 14,
    rowInsets: { top: 1, left: 8, bottom: 1, right: 8 },
    clusterInsets: { left: 8, right: 8 },
    dotSpacing: 4,
    dotTrackHeight: 14,
  },
  /** MahoSidebarSpaceDotView sizes (kViewSize/kIndicatorSize and the dot ramp). */
  spaceDot: {
    hitSize: 15,
    indicatorSize: 11,
    dotIdle: 8,
    dotSelected: 9,
    dotHoveredNoIcon: 11,
    dotActive: 10,
    dotActiveHovered: 12,
    badgeSize: 4,
    hoverAnimationMs: 160,
  },
  /** kSpacesScrollPaddingDp / kHoverTriggerWidthDp. */
  spaces: {
    scrollPadding: 12,
    revealTriggerWidth: 2,
  },
  /** kArchiveEmptyState* dp values. */
  archiveEmptyState: {
    iconSize: 28,
    maxWidth: 252,
    itemSpacing: 8,
  },
} as const;

export type MahoLayoutTokens = typeof MAHO_LAYOUT_TOKENS;

const px = (value: number) => `${value}px`;

/**
 * Flat `--maho-*` layout custom properties, derived from `MAHO_LAYOUT_TOKENS`.
 * `maho.css` declares the same names with the same fallback values;
 * `mahoSpacePalette.test.ts` pins both sides so the token object and the
 * stylesheet cannot drift.
 */
export function mahoLayoutCssVars(): Record<string, string> {
  const { rail, topBar, controlIndicator, searchPill, library, favorites, rows, footer, spaceDot } =
    MAHO_LAYOUT_TOKENS;
  return {
    "--maho-rail-width": px(rail.defaultWidth),
    "--maho-rail-radius": px(rail.cornerRadius),
    "--maho-section-radius": px(rail.sectionCornerRadius),
    "--maho-section-spacing": px(rail.sectionSpacing),
    "--maho-top-region-spacing": px(rail.topRegionSpacing),
    "--maho-content-inset": px(rail.contentInset),
    "--maho-rail-inset-top": px(rail.insets.top),
    "--maho-rail-inset-left": px(rail.insets.left),
    "--maho-rail-inset-bottom": px(rail.insets.bottom),
    "--maho-rail-inset-right": px(rail.insets.right),
    "--maho-row-inset-top": px(rail.rowInsets.top),
    "--maho-row-inset-left": px(rail.rowInsets.left),
    "--maho-row-inset-bottom": px(rail.rowInsets.bottom),
    "--maho-row-inset-right": px(rail.rowInsets.right),
    "--maho-top-bar-height": px(topBar.height),
    "--maho-top-bar-button-size": px(topBar.buttonSize),
    "--maho-top-bar-button-radius": px(topBar.buttonCornerRadius),
    "--maho-top-bar-icon-size": px(topBar.iconSize),
    "--maho-control-indicator-height": px(controlIndicator.height),
    "--maho-control-indicator-radius": px(controlIndicator.cornerRadius),
    "--maho-search-pill-height": px(searchPill.height),
    "--maho-search-pill-radius": px(searchPill.cornerRadius),
    "--maho-library-rail-width": px(library.railWidth),
    "--maho-library-rail-item-width": px(library.itemWidth),
    "--maho-library-rail-item-radius": px(library.itemCornerRadius),
    "--maho-library-rail-item-spacing": px(library.itemSpacing),
    "--maho-library-rail-icon-size": px(library.iconSize),
    "--maho-library-overlay-width": px(library.overlayContentWidth),
    "--maho-gradient-span": px(library.gradientSpan),
    "--maho-favorites-columns": String(favorites.columns),
    "--maho-favorite-tile-height": px(favorites.tileHeight),
    "--maho-favorite-tile-radius": px(favorites.tileCornerRadius),
    "--maho-favorite-tile-spacing": px(favorites.tileSpacing),
    "--maho-favorite-glyph-size": px(favorites.glyphSize),
    "--maho-favorite-glyph-container": px(favorites.glyphContainerSize),
    "--maho-row-height": px(rows.height),
    "--maho-row-radius": px(rows.cornerRadius),
    "--maho-row-icon-size": px(rows.iconSize),
    "--maho-action-row-height": px(rows.actionRowHeight),
    "--maho-footer-row-height": px(footer.rowHeight),
    "--maho-footer-control-size": px(footer.controlSize),
    "--maho-dot-size": px(spaceDot.hitSize),
    "--maho-dot-indicator-size": px(spaceDot.indicatorSize),
  };
}
