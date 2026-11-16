import { readFileSync } from "node:fs";
import { useState } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { computeAccessibleName } from "dom-accessibility-api";
import postcss from "postcss";
import tailwindcss from "tailwindcss";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CommandPalette } from "./components/CommandPalette";
import { AppearanceSection } from "./components/settings/AppearanceSection";
import { AppearanceChatPreview } from "./components/settings/AppearanceChatPreview";
import {
  applyAppearanceSettings, DEFAULT_APPEARANCE_SETTINGS, loadAppearanceSettings,
  saveAppearanceSettings, useAppearanceSettings, type AppearanceAccent,
} from "./lib/appearanceSettings";
import { parseThemeDocument, type ThemeDocumentV1 } from "./lib/themeDocument";
import { RemoteAppearanceButton, RemoteAppearanceSheet } from "./remote/appearance/RemoteAppearanceSheet";

const source = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");
const savedTheme: ThemeDocumentV1 = { version: 1, id: "audit-theme", name: "Audit theme", base: "dark", tokens: {} };
const interactive = 'button, input:not([type="hidden"]), select, textarea, a[href], summary, [role="button"], [role="combobox"], [contenteditable="true"], [tabindex]';
const injectedSheets = new Set<HTMLStyleElement>();
let originalRootAttributes: Array<[string, string]>;

function inject(css: string) {
  const sheet = document.createElement("style");
  sheet.textContent = css;
  document.head.appendChild(sheet);
  injectedSheets.add(sheet);
  return () => { sheet.remove(); injectedSheets.delete(sheet); };
}

function controls(root: HTMLElement) {
  return Array.from(root.querySelectorAll<HTMLElement>(interactive));
}

function isExposed(element: HTMLElement) {
  if (element.closest('[hidden], [inert], [aria-hidden="true"]')) return false;
  for (let node: HTMLElement | null = element; node; node = node.parentElement) {
    const style = getComputedStyle(node);
    if (style.display === "none" || style.visibility === "hidden") return false;
  }
  const details = element.closest("details:not([open])");
  return !details || element === details.querySelector("summary");
}

function tabStops(root: HTMLElement) {
  return controls(root).filter((element) => isExposed(element) && !element.matches(":disabled") && element.tabIndex >= 0);
}

function auditNames(root: HTMLElement) {
  const elements = controls(root).filter((element) => isExposed(element) && element.getAttribute("role") !== "dialog");
  expect(elements.length).toBeGreaterThan(0);
  for (const element of elements) {
    expect(computeAccessibleName(element).trim(), element.outerHTML).not.toBe("");
    if (!element.matches(":disabled")) expect(element.tabIndex, element.outerHTML).toBe(0);
    if (element.tagName === "BUTTON" && !element.textContent?.trim()) {
      expect(element.getAttribute("aria-label")?.trim(), element.outerHTML).toBeTruthy();
    }
  }
}

/** JSDOM has no browser Tab default action. Dispatch real key events first (so product
 * traps can cancel them), then emulate only the uncancelled sequential-focus default.
 * Positive tabindex is sorted as a browser would, not silently treated as DOM order.
 */
function tab(backwards = false) {
  const active = document.activeElement as HTMLElement;
  const event = new KeyboardEvent("keydown", { key: "Tab", shiftKey: backwards, bubbles: true, cancelable: true });
  act(() => {
    if (!active.dispatchEvent(event)) return;
    const ordered = tabStops(document.body).map((element, index) => ({ element, index }))
      .sort((a, b) => (a.element.tabIndex || Infinity) - (b.element.tabIndex || Infinity) || a.index - b.index)
      .map(({ element }) => element);
    const index = ordered.indexOf(active);
    const next = (index + (backwards ? -1 : 1) + ordered.length) % ordered.length;
    ordered[next]?.focus();
  });
}

function auditFocusOrder(root: HTMLElement) {
  const expected = tabStops(root);
  expect(expected.length).toBeGreaterThan(0);
  for (const element of expected) expect(element.tabIndex, element.outerHTML).toBe(0);
  act(() => expected[0].focus());
  expect(document.activeElement).toBe(expected[0]);
  for (const element of expected.slice(1)) {
    tab();
    expect(document.activeElement).toBe(element);
  }
  for (const element of expected.slice(0, -1).reverse()) {
    tab(true);
    expect(document.activeElement).toBe(element);
  }
}

function RemoteHarness() {
  const [open, setOpen] = useState(false);
  return <>
    <RemoteAppearanceButton open={open} onOpen={() => setOpen(true)} />
    <button type="button">Outside sheet</button>
    <RemoteAppearanceSheet open={open} onOpenChange={setOpen} />
  </>;
}

function PaletteHarness() {
  const appearance = useAppearanceSettings();
  const [open, setOpen] = useState(false);
  return <>
    <button type="button" onClick={() => setOpen(true)}>Open palette</button>
    <button type="button">Outside palette</button>
    <CommandPalette open={open} worktrees={[]} tabs={[]} appearance={{ ...appearance, onOpen: vi.fn() }}
      onSelectWorktree={vi.fn()} onSelectTab={vi.fn()} onClose={() => setOpen(false)} />
  </>;
}

beforeEach(() => {
  originalRootAttributes = Array.from(document.documentElement.attributes, ({ name, value }) => [name, value]);
  localStorage.clear();
  saveAppearanceSettings({ ...DEFAULT_APPEARANCE_SETTINGS, customThemes: [savedTheme], activeCustomThemeId: savedTheme.id });
});

afterEach(() => {
  cleanup();
  applyAppearanceSettings(DEFAULT_APPEARANCE_SETTINGS);
  for (const sheet of injectedSheets) sheet.remove();
  injectedSheets.clear();
  for (const attribute of Array.from(document.documentElement.attributes)) document.documentElement.removeAttribute(attribute.name);
  for (const [name, value] of originalRootAttributes) document.documentElement.setAttribute(name, value);
  localStorage.clear();
});

describe("appearance accessibility audit", () => {
  it("names desktop theme library, paste/file import and custom-CSS controls in DOM focus order", () => {
    const { container } = render(<AppearanceSection />);
    fireEvent.click(screen.getByRole("button", { name: "Custom CSS..." }));
    auditNames(container);
    auditFocusOrder(container);
    expect(screen.getByLabelText("Import theme file")).toBeInTheDocument();
    expect(screen.getByLabelText("Theme JSON")).toBeInTheDocument();
    expect(screen.getByLabelText("Standalone custom CSS (applied last)")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove custom CSS" }).closest("[data-chat-scope]")).toBeNull();
  });

  it("names inline rename controls and retains their DOM focus order", () => {
    const { container } = render(<AppearanceSection />);
    fireEvent.click(screen.getByRole("button", { name: "Rename Audit theme" }));
    expect(screen.getByRole("textbox", { name: "Name for Audit theme" })).toBeInTheDocument();
    auditNames(container);
    auditFocusOrder(container);
  });

  it("keeps the real chat preview inert and motionless rather than adding keyboard stops", () => {
    render(<AppearanceChatPreview />);
    const preview = screen.getByTestId("appearance-chat-preview");
    expect(preview.querySelector('[role="group"][inert]')).not.toBeNull();
    expect(tabStops(preview)).toHaveLength(0);
    for (const element of [preview, ...preview.querySelectorAll<HTMLElement>("*")]) {
      expect(getComputedStyle(element).animation).toMatch(/^none\b/);
      expect(getComputedStyle(element).transition).toMatch(/^none\b/);
    }
    // Inert sample controls are not exposed, but must still be named if made interactive later.
    for (const element of controls(preview).filter((element) => element.tagName !== "DIV")) {
      expect(computeAccessibleName(element).trim(), element.outerHTML).not.toBe("");
    }
  });

  it("names remote controls and traverses them in DOM order", () => {
    render(<RemoteHarness />);
    const trigger = screen.getByRole("button", { name: "Appearance" });
    expect(trigger).toHaveAttribute("aria-label", "Appearance");
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Appearance" });
    auditNames(dialog);
    auditFocusOrder(dialog);
  });

  it("traps remote sheet focus in both directions and restores the invoking control on Escape", () => {
    render(<RemoteHarness />);
    const trigger = screen.getByRole("button", { name: "Appearance" });
    act(() => trigger.focus());
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Appearance" });
    const stops = tabStops(dialog);
    expect(document.activeElement).toBe(stops[0]);
    tab(true);
    expect(document.activeElement).toBe(stops.at(-1));
    tab();
    expect(document.activeElement).toBe(stops[0]);
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    // Drive the real exit completion event; never wait for a timing fallback.
    fireEvent.animationEnd(dialog);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(document.activeElement).toBe(trigger);
  });

  it("keeps remote sheet hit areas at least 44px, including the math label target", async () => {
    // Compile the actual utility classes with Tailwind, not a test-only 44px override.
    const raw = source("./remote/appearance/RemoteAppearanceSheet.tsx") + source("./remote/chat/MobileSheet.tsx");
    const css = await postcss([tailwindcss({ content: [{ raw, extension: "tsx" }], corePlugins: { preflight: false } })])
      .process("@tailwind utilities;", { from: undefined });
    const remove = inject(css.css);
    try {
      render(<RemoteHarness />);
      fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
      const dialog = screen.getByRole("dialog", { name: "Appearance" });
      for (const control of tabStops(dialog)) {
        const target = control.matches('input[type="checkbox"]') ? control.closest("label")! : control;
        const style = getComputedStyle(target);
        const pixels = (value: string) => value.endsWith("rem") ? parseFloat(value) * 16 : parseFloat(value) || 0;
        expect(Math.max(pixels(style.height), pixels(style.minHeight)), target.outerHTML).toBeGreaterThanOrEqual(44);
      }
    } finally { remove(); }
  });

  it("names palette search, close and all appearance command rows in DOM focus order", () => {
    render(<PaletteHarness />);
    fireEvent.click(screen.getByRole("button", { name: "Open palette" }));
    const dialog = screen.getByRole("dialog", { name: "Command palette" });
    expect(dialog.querySelectorAll('[data-command-id^="appearance."]')).toHaveLength(3);
    auditNames(dialog);
    auditFocusOrder(dialog);
  });

  it("traps palette focus and restores the opener when dismissed", () => {
    render(<PaletteHarness />);
    const trigger = screen.getByRole("button", { name: "Open palette" });
    act(() => trigger.focus());
    fireEvent.click(trigger);
    const stops = tabStops(screen.getByRole("dialog", { name: "Command palette" }));
    expect(document.activeElement).toBe(stops[0]);
    tab(true);
    expect(document.activeElement).toBe(stops.at(-1));
    tab();
    expect(document.activeElement).toBe(stops[0]);
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(document.activeElement).toBe(trigger);
  });
});

// Independent WCAG sRGB calculation: do not reuse the validator being audited.
function luminance(value: string) {
  let channels: number[];
  if (/^#[\da-f]{6}$/i.test(value)) channels = [1, 3, 5].map((offset) => parseInt(value.slice(offset, offset + 2), 16));
  else if (/^#[\da-f]{3}$/i.test(value)) channels = [...value.slice(1)].map((channel) => parseInt(channel + channel, 16));
  else {
    const match = /^rgb\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)\s*\)$/i.exec(value);
    expect(match, `Expected an opaque computed colour, received ${value}`).not.toBeNull();
    channels = match!.slice(1).map(Number);
  }
  const linear = channels.map((channel) => {
    const s = channel / 255;
    return s <= .04045 ? s / 12.92 : ((s + .055) / 1.055) ** 2.4;
  });
  return .2126 * linear[0] + .7152 * linear[1] + .0722 * linear[2];
}

const accents: AppearanceAccent[] = ["default", "blue", "emerald", "purple", "amber", "rose"];
const matrix = (["dark", "light"] as const).flatMap((theme) => accents.map((accentColor) => ({ theme, accentColor })));

describe("appearance AA contrast enforcement", () => {
  it.each(matrix)("measures semantic computed colour pairs for $theme / $accentColor", ({ theme, accentColor }) => {
    // JSDOM cannot parse Tailwind directives. Retain actual top-level palette rules,
    // selectors, declarations and cascade order; do not substitute fixture colours.
    const palette = postcss.parse(source("./index.css"));
    const rules: string[] = [];
    palette.each((node) => {
      if (node.type === "rule" && node.selector.startsWith(":root")) rules.push(node.toString());
    });
    const remove = inject(rules.join("\n") + "\n" + source("./settings-runtime.css"));
    try {
      applyAppearanceSettings({ ...DEFAULT_APPEARANCE_SETTINGS, theme, accentColor });
      const computed = getComputedStyle(document.documentElement);
      for (const [foreground, background] of [
        ["--chat-primary-foreground", "--chat-primary"],
        ["--chat-foreground", "--chat-surface"],
        // --chat-screen is the shipped canvas token (there is no --chat-background).
        ["--chat-foreground", "--chat-screen"],
      ]) {
        const fg = computed.getPropertyValue(foreground).trim();
        const bg = computed.getPropertyValue(background).trim();
        expect(fg, foreground).not.toBe("");
        expect(bg, background).not.toBe("");
        const a = luminance(fg);
        const b = luminance(bg);
        const ratio = (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
        expect(ratio, `${theme}/${accentColor}: ${foreground} (${fg}) on ${background} (${bg})`).toBeGreaterThanOrEqual(4.5);
      }
    } finally { remove(); }
  });

  it.each([
    ["--chat-foreground", "--chat-surface"],
    ["--chat-foreground", "--chat-screen"],
    ["--chat-primary-foreground", "--chat-primary"],
  ])("refuses imported #777 %s on #888 %s and names both tokens", (foreground, background) => {
    const bad = { ...savedTheme, id: "low-contrast", tokens: { [foreground]: "#777", [background]: "#888" } };
    const parsed = parseThemeDocument(bad);
    expect(parsed.ok).toBe(false);
    if (!parsed.ok) expect(parsed.errors.some((error) => error.path === `tokens.${foreground}`
      && error.message.includes(foreground) && error.message.includes(background))).toBe(true);
    const before = loadAppearanceSettings();
    render(<AppearanceSection />);
    fireEvent.change(screen.getByRole("textbox", { name: "Theme JSON" }), { target: { value: JSON.stringify(bad) } });
    fireEvent.click(screen.getByRole("button", { name: "Import pasted JSON" }));
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent(foreground);
    expect(alert).toHaveTextContent(background);
    expect(loadAppearanceSettings()).toEqual(before);
    expect(loadAppearanceSettings().customThemes.some((theme) => theme.id === bad.id)).toBe(false);
  });
});
