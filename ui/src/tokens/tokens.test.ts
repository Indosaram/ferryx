import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { renderTokenCss, TOKEN_CSS_FILES } from "../../scripts/gen-token-css";
import { DEFAULT_TERMINAL_THEME } from "../lib/terminalSettings";
import { colors, motion, spacing } from ".";

// Vite rewrites `new URL(template, import.meta.url)` into an asset glob, so resolve from __dirname.
const uiRoot = resolve(__dirname, "../..");

function readUi(file: string): string {
  return readFileSync(resolve(uiRoot, file), "utf8");
}

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((offset) => {
    const channel = parseInt(hex.slice(offset, offset + 2), 16) / 255;
    return channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** Composites `fg` at `alpha` over the opaque `bg`, as the browser paints a translucent fill. */
function over(fg: string, alpha: number, bg: string): string {
  const channel = (hex: string, offset: number) => parseInt(hex.slice(offset, offset + 2), 16);
  return `#${[1, 3, 5]
    .map((offset) => Math.round(channel(fg, offset) * alpha + channel(bg, offset) * (1 - alpha)).toString(16).padStart(2, "0"))
    .join("")}`;
}

describe("design tokens", () => {
  it("the generated token CSS in index.css and settings-runtime.css is current", () => {
    for (const file of Object.keys(TOKEN_CSS_FILES)) {
      const source = readUi(file);
      expect(renderTokenCss(file, source), `${file} is stale; run bun ui/scripts/gen-token-css.ts`).toBe(source);
    }
  });

  it("refuses a CSS file whose token region was removed", () => {
    const source = readUi("src/settings-runtime.css").replace(/\/\* @tokens accent-rose:[\s\S]*?@tokens-end accent-rose \*\//, "");
    expect(() => renderTokenCss("src/settings-runtime.css", source)).toThrow(/missing token region\(s\) accent-rose/);
  });

  it("the terminal default theme is the token palette", () => {
    expect(DEFAULT_TERMINAL_THEME).toEqual({ ...colors.terminalPalettes.default, extendedAnsi: [] });
    expect(colors.terminalPalettes.default.background).toBe(colors.base.terminal);
  });

  it("exports the named spacing and motion values the spec defines", () => {
    expect(spacing).toEqual({ touchTarget: 44, phoneHeader: 56, desktopHeader: 28, gap: 8 });
    expect(motion).toMatchObject({ durationFast: 120, durationEnter: 140, durationSheet: 220, enterOffset: 2 });
  });

  describe.each([
    ["default", colors.base, colors.base],
    ["dark", { ...colors.base, ...colors.themeDark }, colors.base],
    ["light", { ...colors.base, ...colors.themeLight }, { ...colors.base, ...colors.themeLight, ...colors.semanticLight }],
  ] as Array<[string, Record<string, string>, Record<string, string>]>)("%s theme semantic token contrast", (_theme, palette, resolved) => {
    const surfaces = [palette.background, palette.card];
    const badgeText = resolved["size-owner-badge-foreground"]?.startsWith("#")
      ? resolved["size-owner-badge-foreground"]
      : palette["status-warning"];

    it("keeps the size owner badge text at 4.5:1 on its 15% warning fill", () => {
      for (const surface of surfaces) {
        expect(contrast(badgeText, over(palette["status-warning"], 0.15, surface))).toBeGreaterThanOrEqual(4.5);
      }
    });

    it("keeps the detached marker and done glyph at 3:1 as non-text UI", () => {
      for (const surface of surfaces) {
        expect(contrast(palette["status-idle"], surface)).toBeGreaterThanOrEqual(3);
        expect(contrast(palette["status-success"], surface)).toBeGreaterThanOrEqual(3);
      }
    });
  });
});
