import { afterEach, describe, expect, it } from "vitest";

import {
  _resetNativeOsThemeForTest,
  applyAppearanceSettings,
  clearThemeDraftPreview,
  DEFAULT_APPEARANCE_SETTINGS,
  loadAppearanceSettings,
  resolveActiveTerminalPalette,
  resolveActiveThemeChoice,
  saveAppearanceSettings,
  setPublishedEnvironmentThemes,
  setThemeDraftPreview,
} from "./appearanceSettings";
import { applyAppThemeCss, clearAppThemeCss, themeCssLoadsRemoteFiles, themeCssValueFetches } from "./themeCss";
import { parseThemeDocument } from "./themeDocument";
import {
  buildThemePreviewGradient,
  contrastRatio,
  createVividThemeColors,
  CURATED_THEMES,
  parseColorToRgb,
  parseT3ThemeFile,
  previewColorsOf,
  serializeToT3ThemeFile,
  themeColorsToFerryxTokens,
  themeDefinitionToDocument,
} from "./themePalettes";
import { parseAnyThemeImport } from "./vscodeThemeImport";

afterEach(() => {
  localStorage.clear();
  _resetNativeOsThemeForTest();
  clearAppThemeCss();
  applyAppearanceSettings(DEFAULT_APPEARANCE_SETTINGS);
  document.documentElement.removeAttribute("style");
});

describe("T3 Code OKLCH palette engine & curated themes", () => {
  it("derives readable dark and light palettes from canvas + accent seeds", () => {
    for (const [appearance, canvas, accent] of [
      ["dark", "#1a1b26", "#7aa2f7"],
      ["light", "#faf8fb", "#a84370"],
    ] as const) {
      const colors = createVividThemeColors(appearance, canvas, accent);
      const canvasRgb = parseColorToRgb(colors.canvas)!;
      const textRgb = parseColorToRgb(colors.text)!;
      const accentRgb = parseColorToRgb(colors.accent)!;
      const accentFgRgb = parseColorToRgb(colors.accentForeground)!;
      expect(contrastRatio(textRgb, canvasRgb)).toBeGreaterThanOrEqual(7);
      expect(contrastRatio(accentFgRgb, accentRgb)).toBeGreaterThanOrEqual(4.5);

      const tokens = themeColorsToFerryxTokens(colors);
      const doc = parseThemeDocument({
        version: 1,
        id: `vivid-${appearance}`,
        name: `Vivid ${appearance}`,
        base: appearance,
        tokens,
      });
      expect(doc.ok).toBe(true);
    }
  });

  it("ships all 5 curated T3 Code themes with light & dark variants and valid tokens", () => {
    expect(CURATED_THEMES.map((t) => t.id)).toEqual(["t3-chat", "grove", "ocean", "ember", "iris"]);
    for (const theme of CURATED_THEMES) {
      const doc = themeDefinitionToDocument(theme);
      expect(parseThemeDocument(doc).ok).toBe(true);
      expect(doc.variants?.dark || doc.variants?.light).toBeDefined();
      const grad = buildThemePreviewGradient(previewColorsOf(theme.colors, theme.appearance), theme.appearance);
      expect(grad).toContain("radial-gradient");
    }
  });

  it("imports T3 Code shorthand, T3 Code full v1, and VS Code JSONC themes seamlessly", () => {
    const shorthand = parseAnyThemeImport(
      JSON.stringify({
        name: "Nightfall",
        appearance: "dark",
        canvas: "#1a1b26",
        accent: "#7aa2f7",
        colors: { error: "#f7768e" },
        css: '[data-slot="sidebar"] { backdrop-filter: blur(8px); }',
      }),
    );
    expect(shorthand.ok).toBe(true);
    if (!shorthand.ok) return;
    expect(shorthand.format).toBe("t3code");
    expect(shorthand.doc.id).toBe("nightfall");
    expect(shorthand.doc.cssScope).toBe("app");

    const exported = serializeToT3ThemeFile(shorthand.doc);
    const reimported = parseT3ThemeFile(JSON.parse(exported));
    expect(reimported.ok).toBe(true);

    const vscodeJsonc = `{
      // Tokyo Night comment
      "name": "tokyo-night-storm",
      "type": "dark",
      "colors": {
        "editor.background": "#24283b",
        "editor.foreground": "#c0caf5",
        "button.background": "#7aa2f7",
        "sideBar.background": "#1f2335",
        "terminal.background": "#1f2335",
        "panel.border": "#1d202f80",
      },
    }`;
    const vsc = parseAnyThemeImport(vscodeJsonc);
    expect(vsc.ok).toBe(true);
    if (!vsc.ok) return;
    expect(vsc.format).toBe("vscode");
    expect(vsc.doc.name).toBe("Tokyo Night Storm");
    expect(vsc.doc.base).toBe("dark");
    expect(vsc.doc.tokens["--background"]).toBe("#24283b");
    expect(vsc.doc.tokens["--terminal"]).toBe("#1f2335");
  });
});

describe("App-wide CSS sanitizer & appearance runtime parity fixes", () => {
  it("detects and strips remote CSS fetches while keeping data: URLs and app-wide rules", () => {
    expect(themeCssValueFetches('url("https://evil.example/a.png")')).toBe(true);
    expect(themeCssValueFetches("image-set('https://evil.example/a.png' 1x)")).toBe(true);
    expect(themeCssValueFetches('url("data:image/png;base64,AAAA")')).toBe(false);
    expect(themeCssValueFetches("url(#clip)")).toBe(false);
    expect(themeCssLoadsRemoteFiles('@import url("https://evil.example/x.css");')).toBe(true);
    expect(themeCssLoadsRemoteFiles('[data-slot="sidebar"] { opacity: 0.9 !important; }')).toBe(false);

    applyAppThemeCss(
      '@import url("https://evil.example/x.css"); .dark [data-slot="sidebar"] { background: url("https://evil.example/bg.png"); color: rgb(1, 2, 3) !important; }',
    );
    const styleEl = document.querySelector("style[data-ferryx-app-theme-css]");
    if (styleEl) {
      expect(styleEl.textContent).not.toContain("evil.example");
      expect(styleEl.textContent).toContain("color: rgb(1, 2, 3) !important");
    }
    clearAppThemeCss();
    expect(document.querySelector("style[data-ferryx-app-theme-css]")).toBeNull();
  });

  it("updates border/input alpha tokens and mirrors tokens for .remote-app-root, restoring cleanly", () => {
    const doc = themeDefinitionToDocument(CURATED_THEMES[0]);
    saveAppearanceSettings({ customThemes: [doc], activeCustomThemeId: doc.id });
    applyAppearanceSettings(loadAppearanceSettings());

    expect(document.documentElement.style.getPropertyValue("--border-alpha")).toBe("1");
    expect(document.documentElement.style.getPropertyValue("--input-alpha")).toBe("1");
    const mirror = document.querySelector("style[data-custom-theme-remote-mirror]");
    expect(mirror?.textContent).toContain(".remote-app-root");
    expect(mirror?.textContent).toContain("--background:");

    applyAppearanceSettings(DEFAULT_APPEARANCE_SETTINGS);
    expect(document.documentElement.style.getPropertyValue("--border-alpha")).toBe("");
    expect(document.querySelector("style[data-custom-theme-remote-mirror]")).toBeNull();
  });

  it("supports independent light/dark theme halves, draft preview rollback, and terminal palette extraction", () => {
    const lightDoc = themeDefinitionToDocument(CURATED_THEMES[0]);
    const darkDoc = themeDefinitionToDocument(CURATED_THEMES[2]);
    const settings = {
      ...DEFAULT_APPEARANCE_SETTINGS,
      theme: "system" as const,
      customThemes: [],
      themeHalves: { light: lightDoc.id, dark: darkDoc.id },
    };
    const resolved = resolveActiveThemeChoice(settings);
    expect(resolved.doc?.id).toBe(darkDoc.id);
    expect(resolved.appearance).toBe("dark");

    const termPalette = resolveActiveTerminalPalette(settings);
    expect(termPalette?.background).toMatch(/^#[0-9a-f]{6}$/i);

    setThemeDraftPreview(lightDoc, "light");
    expect(document.documentElement.dataset.theme).toBe("light");
    clearThemeDraftPreview();
    expect(document.documentElement.dataset.theme).toBe("charcoal");

    setPublishedEnvironmentThemes([{ doc: darkDoc, hasBackground: true, backgroundRevision: "png:10:1" }]);
    const envChoice = resolveActiveThemeChoice({
      ...DEFAULT_APPEARANCE_SETTINGS,
      activeCustomThemeId: darkDoc.id,
    });
    expect(envChoice.doc?.id).toBe(darkDoc.id);
  });
});
