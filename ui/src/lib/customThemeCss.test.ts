import { afterEach, describe, expect, it } from "vitest";
import { applyAppearanceSettings, DEFAULT_APPEARANCE_SETTINGS, type AppearanceSettingsState } from "./appearanceSettings";
import { scopeCustomCss } from "./customThemeCss";
import type { ThemeDocumentV1 } from "./themeDocument";

const defaults = (): AppearanceSettingsState => ({ ...DEFAULT_APPEARANCE_SETTINGS, customThemes: [] });
const lightTheme = (patch: Partial<ThemeDocumentV1> = {}): ThemeDocumentV1 => ({
  version: 1, id: "paper-theme", name: "Paper", base: "light",
  tokens: { "--background": "#fafafa", "--chat-code": "hsl(120 100% 25%)" }, ...patch,
});
const withTheme = (theme = lightTheme()): AppearanceSettingsState => ({
  ...defaults(), customThemes: [theme], activeCustomThemeId: theme.id,
});

function fixture() {
  const scope = document.createElement("section");
  scope.dataset.testid = "mobile-chat-workspace";
  const button = document.createElement("button");
  button.id = "chat";
  scope.append(button);
  const recovery = document.createElement("button");
  recovery.textContent = "Remove custom CSS";
  document.body.append(scope, recovery);
  return { scope, button, recovery };
}

afterEach(() => {
  applyAppearanceSettings(defaults());
  document.body.replaceChildren();
  document.querySelectorAll("style[data-test-palette]").forEach((style) => style.remove());
  document.documentElement.removeAttribute("style");
});

describe("scopeCustomCss", () => {
  it("prefixes every selector with the exact chat-only scope", () => {
    expect(scopeCustomCss("#chat { color: red }", "x")).toEqual({
      ok: true, css: 'html[data-custom-theme="x"] [data-chat-scope] #chat { color: red }',
    });
    const result = scopeCustomCss("button:hover, .message > strong { color: red }", "x");
    expect(result.ok && result.css).toBe('html[data-custom-theme="x"] [data-chat-scope] button:hover, html[data-custom-theme="x"] [data-chat-scope] .message > strong { color: red }');
  });

  it.each([
    "@import url(https://evil)",
    "@import url(https://evil); button { color: red }",
    "button { background: url(https://evil/x.png) }",
    String.raw`button { background: url(\68 ttps://evil) }`,
    "button { background: image-set('https://evil/x.png' 1x) }",
    "button { position: fixed }", "button { z-index: 99 }",
    ":root { --background: red }", "html button { color: red }", "body { color: red }",
    "button:has(+ .shell) { color: red }", "> button { color: red }",
    "button { color: red !important }", "button { --unsafe: red }",
    "@font-face { font-family: evil; src: url(https://evil) }",
    "@namespace url(https://evil);", "@page { color: red }",
    "@supports (display: grid) { button { color: red } }",
    "@layer evil { button { color: red } }",
    "button { color: red", "button { color: rgb(1 2 3 }", "/* missing end",
  ])("refuses unsafe or malformed input atomically: %s", (css) => {
    const result = scopeCustomCss(css, "x");
    expect(result.ok).toBe(false);
    if (result.ok) throw new Error("Expected rejection");
    expect(result.errors[0].path).toBe("rule 0");
    expect(result.errors[0].message.length).toBeGreaterThan(0);
    expect(result).not.toHaveProperty("css");
  });

  it("accepts data images, quoted punctuation and presentational functions", () => {
    const result = scopeCustomCss('button { background: url("data:image/png;base64,aGVsbG8="); padding: calc(1px + 2px); color: rgb(1 2 3); font-family: "Semi; Colon" }', "x");
    expect(result.ok).toBe(true);
  });

  it("accepts only the reduced-motion media wrapper and scopes its children", () => {
    const result = scopeCustomCss("@media (prefers-reduced-motion: reduce) { button { opacity: .8 } }", "x");
    expect(result.ok && result.css).toContain('html[data-custom-theme="x"] [data-chat-scope] button');
    expect(scopeCustomCss("@media print { button { color: red } }", "x").ok).toBe(false);
  });

  it("never emits the good part of a partially rejected sheet", () => {
    const result = scopeCustomCss("button { color: red } button { position: fixed }", "x");
    expect(result).toMatchObject({ ok: false, errors: [{ path: "rule 1" }] });
    expect(result).not.toHaveProperty("css");
  });

  it("rejects scope injection and oversized CSS", () => {
    expect(scopeCustomCss("button { color: red }", 'x"] body').ok).toBe(false);
    expect(scopeCustomCss(" ".repeat(32769), "x").ok).toBe(false);
  });
});

describe("custom theme application", () => {
  it("keeps the built-in-only path free of custom properties, scope attributes and styles", () => {
    applyAppearanceSettings(defaults());
    expect(document.documentElement.dataset.theme).toBe("charcoal");
    expect(document.documentElement.style.cssText).toBe("color-scheme: dark;");
    expect(document.documentElement.hasAttribute("data-custom-theme")).toBe(false);
    expect(document.querySelector("style[data-custom-theme-css]")).toBeNull();
  });

  it("sets the effective base and derived RGB, then restores previous inline values on deletion", () => {
    const root = document.documentElement;
    root.style.setProperty("--background", "#123456");
    applyAppearanceSettings(withTheme());
    expect(root.dataset.theme).toBe("light");
    expect(root.style.colorScheme).toBe("light");
    expect(root.style.getPropertyValue("--background")).toBe("#fafafa");
    expect(root.style.getPropertyValue("--background-rgb")).toBe("250 250 250");
    expect(root.style.getPropertyValue("--chat-code-rgb")).toBe("0 128 0");
    applyAppearanceSettings(defaults());
    expect(root.dataset.theme).toBe("charcoal");
    expect(root.style.getPropertyValue("--background")).toBe("#123456");
    expect(root.style.getPropertyValue("--background-rgb")).toBe("");
    expect(root.style.getPropertyValue("--chat-code")).toBe("");
  });

  it("invalid tokens never reach the DOM, even through a direct caller", () => {
    const result = applyAppearanceSettings(withTheme(lightTheme({ tokens: { "--radius": "2px", "--background": "#fafafa" } })));
    expect(result?.errors.length).toBeGreaterThan(0);
    expect(document.documentElement.dataset.theme).toBe("charcoal");
    expect(document.documentElement.style.getPropertyValue("--background")).toBe("");
    expect(document.documentElement.style.getPropertyValue("--radius")).toBe("");
  });

  it("refuses the whole theme when its CSS is invalid and installs zero CSS", () => {
    fixture();
    const result = applyAppearanceSettings(withTheme(lightTheme({ css: "button { color: red } @import url(https://evil);" })));
    expect(result?.errors.length).toBeGreaterThan(0);
    expect(document.documentElement.dataset.theme).toBe("charcoal");
    expect(document.documentElement.style.getPropertyValue("--background")).toBe("");
    expect(document.querySelector("style[data-custom-theme-css]")).toBeNull();
  });

  it("hides buttons only within chat, never the recovery control", () => {
    const { scope, button, recovery } = fixture();
    const before = getComputedStyle(recovery).display;
    applyAppearanceSettings({ ...defaults(), customCss: "button { display: none }" });
    expect(scope.hasAttribute("data-chat-scope")).toBe(true);
    expect(getComputedStyle(button).display).toBe("none");
    expect(getComputedStyle(recovery).display).toBe(before);
    applyAppearanceSettings(defaults());
    expect(scope.hasAttribute("data-chat-scope")).toBe(false);
    expect(getComputedStyle(button).display).not.toBe("none");
  });

  it("marks chat mounted after activation using the mutation event, without polling", async () => {
    applyAppearanceSettings({ ...defaults(), customCss: "button { color: red }" });
    const changed = new Promise<void>((resolve) => {
      const observer = new MutationObserver((records) => {
        if (records.some((record) => record.attributeName === "data-chat-scope")) { observer.disconnect(); resolve(); }
      });
      observer.observe(document.body, { attributes: true, subtree: true });
    });
    const { scope } = fixture();
    await changed;
    expect(scope.hasAttribute("data-chat-scope")).toBe(true);
  }, 1000);

  it("preserves built-in -> theme CSS -> standalone CSS precedence and exact reset", () => {
    const { button } = fixture();
    const palette = document.createElement("style");
    palette.dataset.testPalette = "";
    palette.textContent = "#chat { color: rgb(0, 0, 255) }";
    document.head.append(palette);
    applyAppearanceSettings(defaults());
    const builtin = getComputedStyle(button).color;
    const builtinRoot = document.documentElement.outerHTML.split("<head>")[0];
    const settings = withTheme(lightTheme({ css: "#chat { color: red }" }));
    applyAppearanceSettings(settings);
    expect(getComputedStyle(button).color).toBe("rgb(255, 0, 0)");
    applyAppearanceSettings({ ...settings, customCss: "#chat { color: green }" });
    expect(getComputedStyle(button).color).toBe("rgb(0, 128, 0)");
    applyAppearanceSettings(defaults());
    expect(getComputedStyle(button).color).toBe(builtin);
    expect(document.documentElement.outerHTML.split("<head>")[0]).toBe(builtinRoot);
    expect(document.querySelector("style[data-custom-theme-css]")).toBeNull();
  });
});
