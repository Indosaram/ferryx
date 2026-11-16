import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

import { DEFAULT_APPEARANCE_SETTINGS, loadAppearanceSettings, loadAppearanceSettingsState, saveAppearanceSettings } from "./appearanceSettings";
import { APPEARANCE_SETTINGS_STORAGE_KEY } from "./storageKeys";
import { parseThemeColor, parseThemeDocument, THEME_TOKEN_ALLOWLIST, type ThemeDocumentV1 } from "./themeDocument";

const valid: ThemeDocumentV1 = {
  version: 1, id: "my-theme", name: "My theme", base: "dark",
  tokens: { "--background": "#123456", "--chat-primary": "#346bf1" },
  css: ".message { color: white; }",
};

function memoryStorage(): Storage {
  const values = new Map<string, string>();
  return {
    get length() { return values.size; },
    clear: () => values.clear(),
    getItem: (key) => values.get(key) ?? null,
    key: (index) => [...values.keys()][index] ?? null,
    removeItem: (key) => { values.delete(key); },
    setItem: (key, value) => { values.set(key, value); },
  };
}

function errors(raw: unknown) {
  const result = parseThemeDocument(raw);
  expect(result.ok).toBe(false);
  if (result.ok) throw new Error("Expected a rejected document");
  return result.errors;
}

describe("themeDocument", () => {
  it("round-trips a valid document through parse, store and load unchanged", () => {
    const parsed = parseThemeDocument(valid);
    expect(parsed).toEqual({ ok: true, doc: valid });
    const storage = memoryStorage();
    saveAppearanceSettings({ customThemes: [valid], activeCustomThemeId: valid.id }, storage);
    expect(loadAppearanceSettings(storage)).toEqual({
      ...DEFAULT_APPEARANCE_SETTINGS, customThemes: [valid], activeCustomThemeId: valid.id,
    });
    expect(loadAppearanceSettingsState(storage).invalidThemes).toEqual([]);
  });

  it("reports every invalid field and token without throwing", () => {
    expect(errors({ version: 1, id: "X", name: "", base: "sepia", tokens: { "--nope": "#fff", "--radius": "2px" } })
      .map(({ path }) => path)).toEqual(["id", "name", "base", "tokens.--nope", "tokens.--radius"]);
  });

  it.each(["--radius-lg", "--chat-primary-rgb", "--ease-out-quart", "--border-alpha", "--unknown"])(
    "rejects non-authorable token %s with a field-level explanation", (token) => {
      expect(errors({ ...valid, tokens: { [token]: "#fff" } })).toEqual([
        { path: `tokens.${token}`, message: expect.any(String) },
      ]);
    },
  );

  it("rejects version 2", () => {
    expect(errors({ ...valid, version: 2 })).toContainEqual({ path: "version", message: expect.any(String) });
  });

  it("enforces id, nonblank name and name length boundaries", () => {
    for (const id of ["a", "UPPER", "-theme", "a".repeat(65)]) expect(errors({ ...valid, id })[0].path).toBe("id");
    for (const name of [" ", "a".repeat(65)]) expect(errors({ ...valid, name })[0].path).toBe("name");
    expect(parseThemeDocument({ ...valid, id: "a".repeat(64), name: "a".repeat(64) }).ok).toBe(true);
  });

  it("enforces the CSS cap in UTF-8 bytes, including multibyte text", () => {
    expect(parseThemeDocument({ ...valid, css: "a".repeat(32768) }).ok).toBe(true);
    for (const css of ["a".repeat(32769), "\u00e9".repeat(16385)]) {
      expect(errors({ ...valid, css })[0].path).toBe("css");
    }
  });

  it("reports the token-entry cap in addition to individual invalid keys", () => {
    const tokens = Object.fromEntries(Array.from({ length: 201 }, (_, i) => [`--unknown-${i}`, "#fff"]));
    expect(errors({ ...valid, tokens })).toHaveLength(202);
    expect(errors({ ...valid, tokens })[0].path).toBe("tokens");
  });

  it("set-equals the colour-valued declarations in the two CSS root blocks", () => {
    const css = readFileSync(new URL("../index.css", import.meta.url), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
    const tokens = new Set<string>();
    for (const block of css.matchAll(/:root(?:\[data-theme="light"\])?\s*\{([^}]+)\}/g)) {
      for (const match of block[1].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
        if (/^(?:#[\da-f]+|(?:rgba?|hsla?|oklch)\()/i.test(match[2].trim())) tokens.add(match[1]);
      }
    }
    expect([...THEME_TOKEN_ALLOWLIST].sort()).toEqual([...tokens].sort());
  });

  it.each(["#abc", "#abcd", "#aabbcc", "#aabbccdd", "rgb(1, 2, 3)", "rgba(1, 2, 3, .5)",
    "rgb(20% 30% 40% / 50%)", "hsl(240 100% 50%)", "hsla(240, 100%, 50%, .8)", "oklch(60% .1 250 / .8)"])(
    "accepts supported colour syntax %s", (color) => {
      expect(parseThemeDocument({ ...valid, tokens: { "--background": color } }).ok).toBe(true);
    },
  );

  it.each(["red", "12", "var(--foreground)", "url(https://example.com)", "#12", "rgb(nope)",
    "rgb(1 2 3) trailing", "rgb(1, 2 3)", "rgba(1,2,3)", "hsl(0 50 50)", "oklch(60% .1)", "rgb(1 2 3 / NaN)"])(
    "rejects unsupported or malformed colour %s", (color) => {
      expect(errors({ ...valid, tokens: { "--background": color } })[0].path).toBe("tokens.--background");
    },
  );

  it("parses colours into numeric RGBA for derived RGB companions", () => {
    expect(parseThemeColor("#abc")).toEqual([170, 187, 204, 1]);
    expect(parseThemeColor("rgb(1 2 3 / 50%)")).toEqual([1, 2, 3, .5]);
    expect(parseThemeColor("hsl(240 100% 50%)")).toEqual([0, 0, 255, 1]);
    expect(parseThemeColor("oklch(100% 0 0)")?.slice(0, 3)).toEqual([255, 255, 255]);
    expect(parseThemeColor("var(--bad)")).toBeNull();
  });

  it.each([null, [], 42, "{}", {}])("returns errors for non-documents: %j", (raw) => {
    expect(() => parseThemeDocument(raw)).not.toThrow();
    expect(parseThemeDocument(raw).ok).toBe(false);
  });

  it("rejects wrong field types, non-object tokens and unknown document fields", () => {
    expect(errors({ ...valid, tokens: [], css: 42, surprise: true }).map(({ path }) => path)).toEqual(["surprise", "tokens", "css"]);
    expect(errors({ ...valid, tokens: { "--background": 42 } })[0].path).toBe("tokens.--background");
  });

  it("enforces foreground/surface and foreground/primary contrast with both token names", () => {
    for (const tokens of [
      { "--chat-foreground": "#111", "--chat-surface": "#111" },
      { "--chat-foreground": "#0a0a0a" },
      { "--chat-primary": "#fff", "--chat-primary-foreground": "#fff" },
      { "--chat-foreground": "#ffffff00" },
    ]) {
      const result = errors({ ...valid, tokens });
      expect(result.some(({ path, message }) => path.startsWith("tokens.--chat-") && /--chat-.+--chat-/.test(message))).toBe(true);
    }
  });

  it("does not require primary fill to contrast against surface", () => {
    expect(parseThemeDocument({ ...valid, tokens: { "--chat-primary": "#346bf1" } }).ok).toBe(true);
    expect(parseThemeDocument({ ...valid, base: "light", tokens: {} }).ok).toBe(true);
  });
});

describe("themeDocument appearance persistence diagnostics", () => {
  it("keeps an invalid stored theme and its errors while falling back to built-in", () => {
    const storage = memoryStorage();
    const raw = { ...valid, tokens: { "--radius-lg": "2px" } };
    storage.setItem(APPEARANCE_SETTINGS_STORAGE_KEY, JSON.stringify({ theme: "light", customThemes: [raw], activeCustomThemeId: valid.id }));
    const before = storage.getItem(APPEARANCE_SETTINGS_STORAGE_KEY);
    const result = loadAppearanceSettingsState(storage);
    expect(result.settings.activeCustomThemeId).toBeNull();
    expect(result.settings.theme).toBe("light");
    expect(result.invalidThemes).toEqual([{ raw, errors: errors(raw) }]);
    expect(storage.getItem(APPEARANCE_SETTINGS_STORAGE_KEY)).toBe(before);
    saveAppearanceSettings({ renderMath: false }, storage);
    expect(loadAppearanceSettingsState(storage).invalidThemes).toEqual(result.invalidThemes);
  });

  it("retains malformed stored JSON verbatim across unrelated settings writes", () => {
    const storage = memoryStorage();
    storage.setItem(APPEARANCE_SETTINGS_STORAGE_KEY, "{broken");
    const result = loadAppearanceSettingsState(storage);
    expect(result.settings).toEqual(DEFAULT_APPEARANCE_SETTINGS);
    expect(result.invalidThemes).toEqual([{ raw: "{broken", errors: [{ path: "$", message: expect.any(String) }] }]);
    saveAppearanceSettings({ accentColor: "rose" }, storage);
    expect(loadAppearanceSettingsState(storage).invalidThemes).toEqual(result.invalidThemes);
  });

  it("retains malformed collections, duplicate ids and invalid themes supplied to save", () => {
    const storage = memoryStorage();
    storage.setItem(APPEARANCE_SETTINGS_STORAGE_KEY, JSON.stringify({ customThemes: "broken" }));
    expect(loadAppearanceSettingsState(storage).invalidThemes[0].raw).toBe("broken");
    storage.clear();
    saveAppearanceSettings({ customThemes: [valid, { ...valid, name: "Duplicate" }] }, storage);
    expect(loadAppearanceSettingsState(storage).invalidThemes[0].errors[0].path).toBe("id");
    expect(loadAppearanceSettings(storage).customThemes).toEqual([valid]);
    saveAppearanceSettings({ customThemes: [{ ...valid, name: "" }] }, storage);
    expect(loadAppearanceSettingsState(storage).invalidThemes.some(({ raw }) => (raw as ThemeDocumentV1).name === "")).toBe(true);
  });

  it("defaults math on, persists new fields, and clears dangling selection", () => {
    const storage = memoryStorage();
    expect(loadAppearanceSettings(storage).renderMath).toBe(true);
    saveAppearanceSettings({ customCss: ".message { opacity: .9 }", renderMath: false, activeCustomThemeId: "missing" }, storage);
    expect(loadAppearanceSettings(storage)).toMatchObject({ customCss: ".message { opacity: .9 }", renderMath: false, activeCustomThemeId: null });
  });
});
