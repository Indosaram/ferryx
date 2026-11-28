// Maho Space palette derivation, ported from Maho Browser's sidebar palette
// resolver: `maho-chromium/browser/ui/theme/maho_theme_helper.cc`
// (`ResolveMahoSidebarPalette`) and its contract in
// `maho-chromium/browser/ui/theme/maho_sidebar_palette_unittest.cc`.
//
// What is ported, one to one:
//   * Base surfaces: kSidebarBackgroundDark `#272336` / kSidebarBackgroundLight
//     `#e4e5ee`.
//   * Tinted stops: Maho's `MakeTintedStop` — the Space HSV tint blended into the
//     base at the Zen kind blend (0.09, the default-opacity value of
//     `SidebarBlendForOpacity`). Maho lays Zen stops at hue −50 / 0 / +50; the port
//     keeps the outer two as `gradientFrom` / `gradientTo` and the middle one as
//     `surface`.
//   * Light family: saturation * 0.85 (capped 0.95), value * 1.08. Dark family:
//     value + 0.02, saturation unchanged.
//   * Row hover: white at alpha 26 over the surface (dark), black at alpha 20
//     (light) — Maho's `row_hover`.
//   * Readable roles: Maho's `ResolveRoleColors` candidates (`#f3f3f7` / `#2d334a`
//     primary, `#8f8f9f` / `#8088a6` secondary), walked toward the family's contrast
//     endpoint until they clear the threshold on every surface they can land on.
//
// Thresholds: primary and muted text hold 4.5:1 (WCAG AA, and Maho's
// `kMinimumReadableContrastRatio`) against all four surfaces; the accent holds
// 3:1 (Maho's `kMinimumVisibleContrastRatio`, the rule for functional glyphs and
// essential boundaries — an accent is not body text).
//
// Determinism: pure arithmetic on the inputs — no clock, no randomness, no locale
// formatting, no CSS parsing. Same (hex, scheme) always yields the same palette.
//
// Scope: these values paint a `[data-maho-space]` container only. They never
// redefine a Ferryx global theme token; `maho.css` falls back to those tokens
// whenever a Space palette is absent.

import { contrastRatio, mixRgb, rgbToHex, type ThemeRgbColor } from "../../../lib/themePalettes";

export type MahoScheme = "light" | "dark";

export type MahoSpacePalette = Readonly<{
  /** Top-left stop of the Space gradient (Space hue − 50°). */
  gradientFrom: string;
  /** Bottom-right stop of the Space gradient (Space hue + 50°). */
  gradientTo: string;
  /** Base tinted surface (the gradient midpoint in Maho's sampler). */
  surface: string;
  /** Resting hover/selection backing over `surface`. */
  surfaceHover: string;
  rowActive: string;
  rowSelected: string;
  neutralGlyph: string;
  /** Primary readable text: >= 4.5:1 against every surface above. */
  foreground: string;
  /** Secondary/muted text: >= 4.5:1 against every surface above. */
  mutedForeground: string;
  /** Space accent glyph/indicator: >= 3:1 against `surface` and `surfaceHover`. */
  accent: string;
}>;

export type MahoSpacePreset = Readonly<{
  id: string;
  label: string;
  /** The preset's color as `#rrggbb`. */
  hex: string;
  /** Maho's source triple, kept for the transcription test. */
  hue: number;
  saturation: number;
  brightness: number;
}>;

/**
 * Maho's Space color presets, transcribed from `kColorPresets` in
 * `maho-chromium/browser/ui/views/sidebar/maho_sidebar_create_space_view.cc`
 * as (hue, saturation, brightness). Hex is the 1x conversion of that triple.
 *
 * Maho ships nine swatches in this order, including the near-neutral Paper
 * anchor. Its default selection is index 7 (blue), which is `sky` here.
 */
export const SPACE_PRESETS: readonly MahoSpacePreset[] = [
  { id: "paper", label: "Paper", hex: "#f2e4ce", hue: 0.10, saturation: 0.15, brightness: 0.95 },
  { id: "rose", label: "Rose", hex: "#e62e86", hue: 0.92, saturation: 0.8, brightness: 0.9 },
  { id: "grape", label: "Grape", hex: "#8a2ee6", hue: 0.75, saturation: 0.8, brightness: 0.9 },
  { id: "cherry", label: "Cherry", hex: "#e62e2e", hue: 0.0, saturation: 0.8, brightness: 0.9 },
  { id: "tangerine", label: "Tangerine", hex: "#e6862e", hue: 0.08, saturation: 0.8, brightness: 0.9 },
  { id: "citron", label: "Citron", hex: "#e6d32e", hue: 0.15, saturation: 0.8, brightness: 0.9 },
  { id: "lime", label: "Lime", hex: "#2ee640", hue: 0.35, saturation: 0.8, brightness: 0.9 },
  { id: "sky", label: "Sky", hex: "#2e8de6", hue: 0.58, saturation: 0.8, brightness: 0.9 },
  { id: "slate", label: "Slate", hex: "#626e8c", hue: 0.62, saturation: 0.3, brightness: 0.55 },
];

/** The preset Space colors as plain hex strings (`#rrggbb`). */
export const SPACE_PRESET_COLORS: readonly string[] = SPACE_PRESETS.map((preset) => preset.hex);

/** Maho's default swatch (blue, `kColorPresets` index 7), mapped onto this list. */
export const DEFAULT_SPACE_PRESET_ID = "sky";

/** kSidebarBackgroundDark / kSidebarBackgroundLight. */
const BASE_SURFACE: Readonly<Record<MahoScheme, ThemeRgbColor>> = {
  dark: { r: 0x27, g: 0x23, b: 0x36 },
  light: { r: 0xe4, g: 0xe5, b: 0xee },
};

/** Maho's primary/secondary role candidates, before contrast resolution. */
const PRIMARY_CANDIDATE: Readonly<Record<MahoScheme, ThemeRgbColor>> = {
  dark: { r: 0xf3, g: 0xf3, b: 0xf7 },
  light: { r: 0x2d, g: 0x33, b: 0x4a },
};
const SECONDARY_CANDIDATE: Readonly<Record<MahoScheme, ThemeRgbColor>> = {
  dark: { r: 0x8f, g: 0x8f, b: 0x9f },
  light: { r: 0x80, g: 0x88, b: 0xa6 },
};

/** Accent seeds for the neutral (invalid-hex) fallback. */
const NEUTRAL_ACCENT: Readonly<Record<MahoScheme, ThemeRgbColor>> = {
  dark: { r: 0x8f, g: 0x8f, b: 0x9f },
  light: { r: 0x6b, g: 0x72, b: 0x80 },
};

const WHITE: ThemeRgbColor = { r: 255, g: 255, b: 255 };
const BLACK: ThemeRgbColor = { r: 0, g: 0, b: 0 };

/** Maho's row_hover alphas: ARGB(26,255,255,255) dark, ARGB(20,0,0,0) light. */
const HOVER_OVERLAY_ALPHA: Readonly<Record<MahoScheme, number>> = { dark: 26 / 255, light: 20 / 255 };

/** Zen stops sit at hue ±50° around the Space color. */
const GRADIENT_HUE_SPREAD = 50;

/** `SidebarBlendForOpacity(kZen, kThemeOpacityDefault)` — the default tint share. */
const SURFACE_TINT_BLEND = 0.09;

const MIN_READABLE_CONTRAST = 4.5;
const MIN_VISIBLE_CONTRAST = 3;

/**
 * Headroom above the WCAG floor. `contrastRatio` runs on the rounded 8-bit value
 * that ships, and the suite asserts the published hex; 0.06 absorbs the last
 * rounding step instead of landing on 4.5000 exactly.
 */
const CONTRAST_MARGIN = 0.06;

const CONTRAST_STEPS = 64;

type Hsv = Readonly<{ h: number; s: number; v: number }>;

/**
 * Strict `#rgb` / `#rrggbb` parser. Maho's own `ParseHexColor` accepts exactly
 * seven characters, so anything else (named colors, `rgb()`, `#rrggbbaa`, typos,
 * empty strings) is invalid and falls back to the neutral palette.
 */
export function parseMahoHex(value: string): ThemeRgbColor | null {
  const match = /^#([\da-f]{3}|[\da-f]{6})$/i.exec(value.trim());
  if (!match) return null;
  const digits = match[1].length === 3 ? [...match[1]].map((digit) => digit + digit).join("") : match[1];
  return {
    r: Number.parseInt(digits.slice(0, 2), 16),
    g: Number.parseInt(digits.slice(2, 4), 16),
    b: Number.parseInt(digits.slice(4, 6), 16),
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** Snap to 8-bit channels so the value proven for contrast is the value emitted. */
function quantize(color: ThemeRgbColor): ThemeRgbColor {
  return { r: Math.round(color.r), g: Math.round(color.g), b: Math.round(color.b) };
}

function rgbToHsv(rgb: ThemeRgbColor): Hsv {
  const r = rgb.r / 255;
  const g = rgb.g / 255;
  const b = rgb.b / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const delta = max - min;
  let hue = 0;
  if (delta !== 0) {
    if (max === r) hue = ((g - b) / delta) % 6;
    else if (max === g) hue = (b - r) / delta + 2;
    else hue = (r - g) / delta + 4;
    hue *= 60;
  }
  return { h: ((hue % 360) + 360) % 360, s: max === 0 ? 0 : delta / max, v: max };
}

function hsvToRgb(hsv: Hsv): ThemeRgbColor {
  const hue = ((((hsv.h % 360) + 360) % 360) / 60) % 6;
  const saturation = clamp(hsv.s, 0, 1);
  const value = clamp(hsv.v, 0, 1);
  const chroma = value * saturation;
  const x = chroma * (1 - Math.abs((hue % 2) - 1));
  const m = value - chroma;
  const sector = Math.floor(hue) % 6;
  const [r, g, b] =
    sector === 0
      ? [chroma, x, 0]
      : sector === 1
        ? [x, chroma, 0]
        : sector === 2
          ? [0, chroma, x]
          : sector === 3
            ? [0, x, chroma]
            : sector === 4
              ? [x, 0, chroma]
              : [chroma, 0, x];
  return {
    r: Math.round((r + m) * 255),
    g: Math.round((g + m) * 255),
    b: Math.round((b + m) * 255),
  };
}

/** Maho's `MakeTintedStop` family adjustment, minus the blend step. */
function tintedStop(hsv: Hsv, scheme: MahoScheme): ThemeRgbColor {
  if (scheme === "dark") {
    return hsvToRgb({ h: hsv.h, s: hsv.s, v: clamp(hsv.v + 0.02, 0, 1) });
  }
  return hsvToRgb({ h: hsv.h, s: clamp(hsv.s * 0.85, 0, 0.95), v: clamp(hsv.v * 1.08, 0, 1) });
}

function blendTint(base: ThemeRgbColor, tint: ThemeRgbColor): ThemeRgbColor {
  return mixRgb(base, tint, SURFACE_TINT_BLEND);
}

function hoverOverlay(scheme: MahoScheme): ThemeRgbColor {
  return scheme === "dark" ? WHITE : BLACK;
}

/**
 * Maho's `color_utils::BlendForMinContrast`: walk from `candidate` toward
 * `endpoint` until it clears `minimum` on every surface. The walk is monotonic, so
 * the first passing step is stable across runs and machines.
 */
function resolveForContrast(
  candidate: ThemeRgbColor,
  surfaces: readonly ThemeRgbColor[],
  endpoint: ThemeRgbColor,
  minimum: number,
): ThemeRgbColor {
  const threshold = minimum + CONTRAST_MARGIN;
  const passes = (color: ThemeRgbColor) => surfaces.every((surface) => contrastRatio(color, surface) >= threshold);
  const start = quantize(candidate);
  if (passes(start)) return start;
  for (let step = 1; step <= CONTRAST_STEPS; step += 1) {
    const next = quantize(mixRgb(start, endpoint, step / CONTRAST_STEPS));
    if (passes(next)) return next;
  }
  return endpoint;
}

function buildPalette(
  raw: {
    gradientFrom: ThemeRgbColor;
    gradientTo: ThemeRgbColor;
    surface: ThemeRgbColor;
    surfaceHover: ThemeRgbColor;
    accentCandidate: ThemeRgbColor;
  },
  scheme: MahoScheme,
): MahoSpacePalette {
  const endpoint = scheme === "dark" ? WHITE : BLACK;
  const gradientFrom = quantize(raw.gradientFrom);
  const gradientTo = quantize(raw.gradientTo);
  const surface = quantize(raw.surface);
  const surfaceHover = quantize(raw.surfaceHover);
  const overlay = hoverOverlay(scheme);
  const rowActive = quantize(mixRgb(surface, overlay, (scheme === "dark" ? 51 : 38) / 255));
  const rowSelected = quantize(mixRgb(surface, overlay, (scheme === "dark" ? 38 : 25) / 255));
  const all = [gradientFrom, gradientTo, surface, surfaceHover, rowActive, rowSelected];

  return {
    gradientFrom: rgbToHex(gradientFrom),
    gradientTo: rgbToHex(gradientTo),
    surface: rgbToHex(surface),
    surfaceHover: rgbToHex(surfaceHover),
    rowActive: rgbToHex(rowActive),
    rowSelected: rgbToHex(rowSelected),
    neutralGlyph: rgbToHex(resolveForContrast(SECONDARY_CANDIDATE[scheme], all, endpoint, scheme === "dark" ? MIN_VISIBLE_CONTRAST : MIN_READABLE_CONTRAST)),
    foreground: rgbToHex(resolveForContrast(PRIMARY_CANDIDATE[scheme], all, endpoint, MIN_READABLE_CONTRAST)),
    mutedForeground: rgbToHex(resolveForContrast(SECONDARY_CANDIDATE[scheme], all, endpoint, MIN_READABLE_CONTRAST)),
    accent: rgbToHex(resolveForContrast(quantize(raw.accentCandidate), [surface, surfaceHover], endpoint, MIN_VISIBLE_CONTRAST)),
  };
}

function neutralPalette(scheme: MahoScheme): MahoSpacePalette {
  const surface = BASE_SURFACE[scheme];
  return buildPalette(
    {
      gradientFrom: surface,
      gradientTo: surface,
      surface,
      surfaceHover: mixRgb(surface, hoverOverlay(scheme), HOVER_OVERLAY_ALPHA[scheme]),
      accentCandidate: NEUTRAL_ACCENT[scheme],
    },
    scheme,
  );
}

/**
 * Resolve a Space color into a full sidebar palette for one appearance.
 *
 * `hex` is the Space's primary color (any `#rgb` / `#rrggbb`). Anything else —
 * empty, a named color, a malformed string — resolves to the neutral palette built
 * from Maho's base surface and role candidates, so a corrupt persisted value still
 * paints a readable Space instead of throwing.
 */
export function deriveSpacePalette(hex: string, scheme: MahoScheme): MahoSpacePalette {
  const rgb = parseMahoHex(hex);
  if (!rgb) return neutralPalette(scheme);

  const hsv = rgbToHsv(rgb);
  const base = BASE_SURFACE[scheme];
  const surface = blendTint(base, tintedStop(hsv, scheme));

  return buildPalette(
    {
      gradientFrom: blendTint(base, tintedStop({ ...hsv, h: hsv.h - GRADIENT_HUE_SPREAD }, scheme)),
      gradientTo: blendTint(base, tintedStop({ ...hsv, h: hsv.h + GRADIENT_HUE_SPREAD }, scheme)),
      surface,
      surfaceHover: mixRgb(surface, hoverOverlay(scheme), HOVER_OVERLAY_ALPHA[scheme]),
      accentCandidate: hsvToRgb({
        h: hsv.h,
        s: clamp(hsv.s * 1.15 + 0.2, 0.45, 0.85),
        v: scheme === "dark" ? 0.62 : 0.48,
      }),
    },
    scheme,
  );
}

/**
 * CSS custom properties for a `[data-maho-space]` container, in kebab-case so they
 * drop straight into a style object. `maho.css` consumes exactly these names; any
 * surface without them falls back to the Ferryx semantic tokens (`--sidebar`,
 * `--sidebar-foreground`, `--surface-hover`, ...).
 */
export function spacePaletteCssVars(palette: MahoSpacePalette): Record<string, string> {
  return {
    "--maho-space-gradient": `linear-gradient(135deg, ${palette.gradientFrom}, ${palette.gradientTo})`,
    "--maho-space-gradient-from": palette.gradientFrom,
    "--maho-space-gradient-to": palette.gradientTo,
    "--maho-space-surface": palette.surface,
    "--maho-space-surface-hover": palette.surfaceHover,
    "--maho-space-row-active": palette.rowActive,
    "--maho-space-row-selected": palette.rowSelected,
    "--maho-space-neutral-glyph": palette.neutralGlyph,
    "--maho-space-outline": palette.neutralGlyph,
    "--maho-space-foreground": palette.foreground,
    "--maho-space-muted-foreground": palette.mutedForeground,
    "--maho-space-accent": palette.accent,
  };
}

/** The container attribute that scopes a Space theme. */
export const MAHO_SPACE_ATTRIBUTE = "data-maho-space";

/**
 * Spread onto a `[data-maho-space]` element's `style`: `<div data-maho-space="space-id" style={...}>`.
 * Returning the vars rather than mutating a node keeps this pure and testable while
 * still being a one-liner at the call site.
 */
export function spacePaletteStyle(hex: string, scheme: MahoScheme): Record<string, string> {
  return spacePaletteCssVars(deriveSpacePalette(hex, scheme));
}
