import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { contrastRatio } from "../../../lib/themePalettes";
import { MAHO_LAYOUT_TOKENS, mahoLayoutCssVars } from "./mahoTokens";
import {
  DEFAULT_SPACE_PRESET_ID,
  SPACE_PRESETS,
  SPACE_PRESET_COLORS,
  deriveSpacePalette,
  parseMahoHex,
  spacePaletteCssVars,
  spacePaletteStyle,
  type MahoScheme,
  type MahoSpacePalette,
} from "./mahoSpacePalette";

const SCHEMES: readonly MahoScheme[] = ["light", "dark"];

/** WCAG AA for body text, and Maho's `kMinimumReadableContrastRatio`. */
const READABLE_TEXT = 4.5;
/** Maho's `kMinimumVisibleContrastRatio`: functional glyphs and essential boundaries. */
const VISIBLE_GLYPH = 3;

function readCss(): string {
  // Resolved from the ui/ working directory, not via `new URL(..., import.meta.url)`:
  // Vite rewrites a static URL literal like that into an asset reference, which is
  // not a `file:` path under the jsdom test environment.
  return readFileSync(join(process.cwd(), "src", "components", "browser", "maho", "maho.css"), "utf8");
}

function rgb(hex: string) {
  const parsed = parseMahoHex(hex);
  if (!parsed) throw new Error(`fixture is not a hex color: ${hex}`);
  return parsed;
}

function surfacesOf(palette: MahoSpacePalette): string[] {
  return [palette.gradientFrom, palette.gradientTo, palette.surface, palette.surfaceHover];
}

describe("maho space presets", () => {
  it("ships eight unique, valid Space colors", () => {
    expect(SPACE_PRESETS).toHaveLength(8);
    expect(SPACE_PRESET_COLORS).toEqual(SPACE_PRESETS.map((preset) => preset.hex));
    expect(new Set(SPACE_PRESET_COLORS).size).toBe(SPACE_PRESET_COLORS.length);
    for (const hex of SPACE_PRESET_COLORS) {
      expect(hex).toMatch(/^#[0-9a-f]{6}$/);
      expect(parseMahoHex(hex)).not.toBeNull();
    }
  });

  it("keeps each preset's hex consistent with its Maho HSV triple", () => {
    // The presets are transcribed from Maho's kColorPresets as
    // (hue, saturation, brightness); the hex is the 1x conversion of that triple, so
    // a transcription slip shows up as hue/brightness/saturation drift.
    for (const preset of SPACE_PRESETS) {
      const { r, g, b } = rgb(preset.hex);
      const rn = r / 255;
      const gn = g / 255;
      const bn = b / 255;
      const max = Math.max(rn, gn, bn);
      const min = Math.min(rn, gn, bn);
      const delta = max - min;
      let hue = 0;
      if (delta !== 0) {
        if (max === rn) hue = ((gn - bn) / delta) % 6;
        else if (max === gn) hue = (bn - rn) / delta + 2;
        else hue = (rn - gn) / delta + 4;
        hue = (((hue * 60) % 360) + 360) % 360;
      }
      const expectedHue = preset.hue * 360;
      const circular = Math.min(Math.abs(hue - expectedHue), 360 - Math.abs(hue - expectedHue));
      expect(circular, `${preset.id} hue`).toBeLessThan(2);
      expect(Math.abs(max - preset.brightness), `${preset.id} brightness`).toBeLessThan(0.02);
      if (max > 0) {
        expect(Math.abs(delta / max - preset.saturation), `${preset.id} saturation`).toBeLessThan(0.02);
      }
    }
  });

  it("points the default preset id at a real preset", () => {
    expect(SPACE_PRESETS.some((preset) => preset.id === DEFAULT_SPACE_PRESET_ID)).toBe(true);
  });
});

describe("deriveSpacePalette", () => {
  it("is deterministic for repeated calls with the same input", () => {
    for (const hex of SPACE_PRESET_COLORS) {
      for (const scheme of SCHEMES) {
        const first = deriveSpacePalette(hex, scheme);
        expect(deriveSpacePalette(hex, scheme)).toEqual(first);
        expect(deriveSpacePalette(hex, scheme)).toEqual(first);
      }
    }
  });

  it("normalizes case, surrounding space and short hex to one palette", () => {
    expect(deriveSpacePalette("#FF00FF", "dark")).toEqual(deriveSpacePalette("#f0f", "dark"));
    expect(deriveSpacePalette("  #ff00ff  ", "light")).toEqual(deriveSpacePalette("#f0f", "light"));
  });

  it("returns seven hex colors and a two-stop gradient for every preset", () => {
    for (const hex of SPACE_PRESET_COLORS) {
      for (const scheme of SCHEMES) {
        const palette = deriveSpacePalette(hex, scheme);
        for (const [role, value] of Object.entries(palette)) {
          expect(value, `${hex} ${scheme} ${role}`).toMatch(/^#[0-9a-f]{6}$/);
        }
        expect(palette.gradientFrom).not.toBe(palette.gradientTo);
      }
    }
  });

  it("meets WCAG AA for primary and muted text on every preset surface", () => {
    for (const hex of SPACE_PRESET_COLORS) {
      for (const scheme of SCHEMES) {
        const palette = deriveSpacePalette(hex, scheme);
        for (const surface of surfacesOf(palette)) {
          expect(contrastRatio(rgb(palette.foreground), rgb(surface)), `${hex} ${scheme} fg/${surface}`)
            .toBeGreaterThanOrEqual(READABLE_TEXT);
          expect(contrastRatio(rgb(palette.mutedForeground), rgb(surface)), `${hex} ${scheme} muted/${surface}`)
            .toBeGreaterThanOrEqual(READABLE_TEXT);
        }
      }
    }
  });

  it("keeps the accent visible (>= 3:1) on the surfaces it is drawn on", () => {
    for (const hex of SPACE_PRESET_COLORS) {
      for (const scheme of SCHEMES) {
        const palette = deriveSpacePalette(hex, scheme);
        for (const surface of [palette.surface, palette.surfaceHover]) {
          expect(contrastRatio(rgb(palette.accent), rgb(surface)), `${hex} ${scheme} accent/${surface}`)
            .toBeGreaterThanOrEqual(VISIBLE_GLYPH);
        }
      }
    }
  });

  it("resolves the family the caller asked for", () => {
    const light = deriveSpacePalette("#2e8de6", "light");
    const dark = deriveSpacePalette("#2e8de6", "dark");
    // The light-family surface sits far brighter against white than the dark one.
    expect(contrastRatio(rgb(light.surface), rgb("#ffffff"))).toBeLessThan(
      contrastRatio(rgb(dark.surface), rgb("#ffffff")),
    );
    for (const palette of [light, dark]) {
      expect(contrastRatio(rgb(palette.foreground), rgb(palette.surface))).toBeGreaterThanOrEqual(READABLE_TEXT);
    }
  });

  it("tints toward the Space color instead of returning a flat gray", () => {
    const rose = deriveSpacePalette("#e62e86", "dark");
    const sky = deriveSpacePalette("#2e8de6", "dark");
    const slate = deriveSpacePalette("#626e8c", "dark");
    expect(new Set([rose.surface, sky.surface, slate.surface]).size).toBe(3);
    expect(rose.gradientFrom).not.toBe(sky.gradientFrom);
  });

  it("falls back to the neutral palette for invalid input", () => {
    const invalid = [
      "",
      "   ",
      "nope",
      "red",
      "#12",
      "#12345",
      "#1234567",
      "#12345678",
      "#gggggg",
      "rgb(1,2,3)",
      "hsl(120 50% 50%)",
      "0x123456",
      "##123456",
    ];
    for (const scheme of SCHEMES) {
      const neutral = deriveSpacePalette("", scheme);
      // No tint survives the fallback: every surface is Maho's base surface.
      expect(neutral.surface).toBe(scheme === "dark" ? "#272336" : "#e4e5ee");
      expect(neutral.gradientFrom).toBe(neutral.surface);
      expect(neutral.gradientTo).toBe(neutral.surface);
      // The neutral palette itself is still readable.
      for (const surface of surfacesOf(neutral)) {
        expect(contrastRatio(rgb(neutral.foreground), rgb(surface))).toBeGreaterThanOrEqual(READABLE_TEXT);
        expect(contrastRatio(rgb(neutral.mutedForeground), rgb(surface))).toBeGreaterThanOrEqual(READABLE_TEXT);
      }
      for (const value of invalid) {
        expect(deriveSpacePalette(value, scheme), `fallback for ${JSON.stringify(value)}`).toEqual(neutral);
      }
    }
  });

  it("does not carry state between calls", () => {
    const before = [...SPACE_PRESET_COLORS];
    deriveSpacePalette("#123456", "dark");
    deriveSpacePalette("garbage", "light");
    deriveSpacePalette(SPACE_PRESET_COLORS[3], "dark");
    expect([...SPACE_PRESET_COLORS]).toEqual(before);
  });
});

describe("spacePaletteCssVars", () => {
  it("publishes exactly the documented --maho-space-* custom properties", () => {
    const palette = deriveSpacePalette("#8a2ee6", "dark");
    const vars = spacePaletteCssVars(palette);

    expect(Object.keys(vars).sort()).toEqual([
      "--maho-space-accent",
      "--maho-space-foreground",
      "--maho-space-gradient",
      "--maho-space-gradient-from",
      "--maho-space-gradient-to",
      "--maho-space-muted-foreground",
      "--maho-space-surface",
      "--maho-space-surface-hover",
    ]);
    expect(vars["--maho-space-gradient"]).toBe(
      `linear-gradient(135deg, ${palette.gradientFrom}, ${palette.gradientTo})`,
    );
    expect(vars["--maho-space-gradient-from"]).toBe(palette.gradientFrom);
    expect(vars["--maho-space-gradient-to"]).toBe(palette.gradientTo);
    expect(vars["--maho-space-surface"]).toBe(palette.surface);
    expect(vars["--maho-space-surface-hover"]).toBe(palette.surfaceHover);
    expect(vars["--maho-space-foreground"]).toBe(palette.foreground);
    expect(vars["--maho-space-muted-foreground"]).toBe(palette.mutedForeground);
    expect(vars["--maho-space-accent"]).toBe(palette.accent);
  });

  it("publishes every --maho-space-* variable maho.css consumes", () => {
    const css = readCss();
    const published = new Set(Object.keys(spacePaletteCssVars(deriveSpacePalette("#e62e86", "light"))));
    const consumed = new Set([...css.matchAll(/var\((--maho-space-[a-z0-9-]+)\)/g)].map((match) => match[1]));
    expect(consumed.size).toBeGreaterThan(0);
    for (const name of consumed) {
      expect(published, `maho.css consumes ${name}`).toContain(name);
    }
  });

  it("gives the same vars through the style helper", () => {
    expect(spacePaletteStyle("#8a2ee6", "dark")).toEqual(
      spacePaletteCssVars(deriveSpacePalette("#8a2ee6", "dark")),
    );
  });
});

describe("maho layout tokens", () => {
  it("carries the Maho measurements the sidebar chrome depends on", () => {
    // maho_sidebar_layout_tokens.h and MahoSidebarTabListView constants.
    expect(MAHO_LAYOUT_TOKENS.rows.actionRowHeight).toBe(42);
    expect(MAHO_LAYOUT_TOKENS.library.railWidth).toBe(92);
    expect(MAHO_LAYOUT_TOKENS.library.gradientSpan).toBe(MAHO_LAYOUT_TOKENS.library.overlayContentWidth);
    expect(MAHO_LAYOUT_TOKENS.favorites.columns).toBe(4);
    expect(MAHO_LAYOUT_TOKENS.favorites.capacity).toBe(12);
    expect(MAHO_LAYOUT_TOKENS.favorites.tileHeight).toBe(47);
    expect(MAHO_LAYOUT_TOKENS.rows.height).toBe(36);
    expect(MAHO_LAYOUT_TOKENS.rail.defaultWidth).toBe(288);
    expect(MAHO_LAYOUT_TOKENS.topBar.buttonSize).toBe(20);
    expect(MAHO_LAYOUT_TOKENS.topBar.iconSize).toBe(16);
    expect(MAHO_LAYOUT_TOKENS.spaces.revealTriggerWidth).toBe(2);
  });

  it("keeps the stylesheet fallbacks and the token object in lockstep", () => {
    const css = readCss();
    const block = /\.maho-sidebar\s*\{([\s\S]*?)\}/.exec(css);
    expect(block).not.toBeNull();
    const paletteNames = new Set(Object.keys(spacePaletteCssVars(deriveSpacePalette("#e62e86", "light"))));
    const declared = new Map(
      [...(block?.[1] ?? "").matchAll(/(--maho-[a-z0-9-]+):\s*([^;]+);/g)]
        .filter((match) => !paletteNames.has(match[1]))
        .map((match) => [match[1], match[2].trim()]),
    );
    const derived = mahoLayoutCssVars();
    // Same names, same values: a number changed on either side fails here instead of
    // silently drifting between the TS constant and the stylesheet fallback.
    expect(declared.size).toBe(Object.keys(derived).length);
    for (const [name, value] of Object.entries(derived)) {
      expect(declared.get(name), name).toBe(value);
    }
  });

  it("declares only --maho-* custom properties, never a Ferryx theme token", () => {
    const css = readCss();
    const declarations = [...css.matchAll(/(?:^|[\s;{])(--[a-z0-9-]+)\s*:/gm)].map((match) => match[1]);
    expect(declarations.length).toBeGreaterThan(0);
    for (const name of declarations) {
      expect(name.startsWith("--maho-"), `${name} must stay namespaced`).toBe(true);
    }
    // The global palette roots belong to index.css / settings-runtime.css.
    expect(css).not.toMatch(/:root\s*\{/);
  });
});
