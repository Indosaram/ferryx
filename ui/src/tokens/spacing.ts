/**
 * Named spacing values TS code needs, in CSS px. The Tailwind spacing scale stays the source
 * for class-based layout; these are for code that computes geometry.
 */
export const spacing = {
  /** Minimum phone touch target edge (WCAG 2.5.5). */
  touchTarget: 44,
  /** Phone header height. */
  phoneHeader: 56,
  /** Desktop pane header height. */
  desktopHeader: 28,
  /** Default gap between related controls. */
  gap: 8,
} as const;

/** Root font size per UI density (`:root[data-density]`). */
export const density = {
  compact: { "font-size": "16px" },
  comfortable: { "font-size": "17px" },
} as const;

/** Corner radius base; Tailwind derives lg/md/sm from it. */
export const radius = "0.5rem";
