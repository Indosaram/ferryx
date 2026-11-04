/**
 * Motion tokens. Animate only transform, opacity and filter; every looping or translating
 * animation needs a reduced-motion branch (see usePrefersReducedMotion).
 */
export const motion = {
  /** Small state changes and the reduced-motion crossfade ceiling. */
  durationFast: 120,
  /** The `animate-enter` entrance. */
  durationEnter: 140,
  /** Bottom sheets and drawers. */
  durationSheet: 220,
  easeOut: "cubic-bezier(0.2, 0, 0, 1)",
  /** Vertical offset of the `animate-enter` entrance, in px. */
  enterOffset: 2,
} as const;

export const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";
