import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const accents = ["default", "blue", "emerald", "purple", "amber", "rose"] as const;
const themes = ["dark", "light"] as const;
const brandTokens = ["--chat-primary", "--chat-primary-foreground", "--chat-link", "--chat-code"] as const;
// Vitest runs with cwd = ui/, so resolve from there; import.meta.url is not a
// file: URL under the jsdom transform and throws before any test runs.
const indexCss = readFileSync(resolve(process.cwd(), "src/index.css"), "utf8");
const runtimeCss = readFileSync(resolve(process.cwd(), "src/settings-runtime.css"), "utf8");

// Mount the shipped root rules, not a parallel palette fixture. Exclude font/import
// and Tailwind directives that need Vite processing, and component rules that do
// not participate in the root token cascade.
function rootRules(css: string): string {
  return css.replace(/\/\*[\s\S]*?\*\//g, "").match(/:root(?:\[[^\]]+\])*\s*\{[^}]*\}/g)?.join("\n") ?? "";
}

function probe(theme: string, accent: string): CSSStyleDeclaration {
  document.documentElement.dataset.theme = theme;
  document.documentElement.dataset.accent = accent;
  return getComputedStyle(document.documentElement);
}

function value(style: CSSStyleDeclaration, name: string): string {
  return style.getPropertyValue(name).trim().toLowerCase();
}

function channels(hex: string): number[] {
  expect(hex).toMatch(/^#[0-9a-f]{6}$/);
  return [1, 3, 5].map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16));
}

function luminance(hex: string): number {
  const [r, g, b] = channels(hex).map((part) => {
    const channel = part / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const hi = Math.max(luminance(a), luminance(b));
  const lo = Math.min(luminance(a), luminance(b));
  return (hi + 0.05) / (lo + 0.05);
}

describe("chat accent root cascade", () => {
  let stylesheet: HTMLStyleElement;
  let previousTheme: string | undefined;
  let previousAccent: string | undefined;

  beforeEach(() => {
    previousTheme = document.documentElement.dataset.theme;
    previousAccent = document.documentElement.dataset.accent;
    stylesheet = document.createElement("style");
    stylesheet.textContent = `${rootRules(indexCss)}\n${rootRules(runtimeCss)}`;
    document.head.append(stylesheet);
  });

  afterEach(() => {
    stylesheet.remove();
    if (previousTheme === undefined) delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = previousTheme;
    if (previousAccent === undefined) delete document.documentElement.dataset.accent;
    else document.documentElement.dataset.accent = previousAccent;
  });

  describe.each(themes)("%s theme", (theme) => {
    it("computes six distinct primary fills from the active accent", () => {
      const fills = accents.map((accent) => value(probe(theme, accent), "--chat-primary"));
      expect(new Set(fills).size).toBe(accents.length);
    });

    it("preserves every default brand token from the shipped base palette", () => {
      document.documentElement.dataset.theme = theme;
      delete document.documentElement.dataset.accent;
      const base = getComputedStyle(document.documentElement);
      const defaults = brandTokens.map((token) => [value(base, token), value(base, `${token}-rgb`)]);
      const selected = probe(theme, "default");
      expect(value(selected, "--chat-primary")).toBe(theme === "light" ? "#1d4ed8" : "#346bf1");
      expect(brandTokens.map((token) => [value(selected, token), value(selected, `${token}-rgb`)])).toEqual(defaults);
    });

    it.each(accents)("keeps %s foreground readable and every brand RGB companion in sync", (accent) => {
      const style = probe(theme, accent);
      expect(contrast(value(style, "--chat-primary-foreground"), value(style, "--chat-primary"))).toBeGreaterThanOrEqual(4.5);
      for (const token of brandTokens) {
        expect(value(style, `${token}-rgb`).split(/\s+/).map(Number)).toEqual(channels(value(style, token)));
      }
    });

    it("does not leave rose brand colours at their default blue values", () => {
      const defaults = probe(theme, "default");
      const blueValues = ["--chat-primary", "--chat-link", "--chat-code"].map((token) => value(defaults, token));
      const rose = probe(theme, "rose");
      for (const token of ["--chat-primary", "--chat-link", "--chat-code"]) {
        expect(blueValues).not.toContain(value(rose, token));
      }
    });

    it("leaves all non-brand chat tokens unchanged when switching accents", () => {
      const chatTokens = [...indexCss.matchAll(/(--chat-[\w-]+)\s*:/g)].map((match) => match[1]);
      const neutralTokens = [...new Set(chatTokens)].filter((token) =>
        !brandTokens.some((brand) => token === brand || token === `${brand}-rgb`),
      );
      const base = probe(theme, "default");
      const defaults = neutralTokens.map((token) => value(base, token));
      for (const accent of accents) {
        const style = probe(theme, accent);
        expect(neutralTokens.map((token) => value(style, token))).toEqual(defaults);
      }
    });
  });

  it("preserves the default primary in the charcoal palette", () => {
    expect(value(probe("charcoal", "default"), "--chat-primary")).toBe("#346bf1");
  });
});
