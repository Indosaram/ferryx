import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  loadShortcutOverrides,
  loadShortcutPreset,
  nativeShortcutTableFor,
  saveShortcutOverrides,
  saveShortcutPreset,
} from "./shortcutOverrides";
import { SHORTCUTS, resolvedShortcuts, shortcutLabel, useShortcuts, type ShortcutActionId } from "./shortcuts";
import { PROJECTS_STORAGE_KEY, SHORTCUT_OVERRIDES_STORAGE_KEY, SHORTCUT_PRESET_STORAGE_KEY } from "./storageKeys";

type Handlers = Partial<Record<ShortcutActionId, ReturnType<typeof vi.fn>>>;

function tabHandlers(): Handlers {
  return {
    "tab.select1": vi.fn(),
    "tab.select2": vi.fn(),
    "tab.next": vi.fn(),
    "tab.previous": vi.fn(),
    "workspace.select1": vi.fn(),
  };
}

function mountTerminal(): { terminal: HTMLTextAreaElement; remove: () => void } {
  const host = document.createElement("div");
  host.className = "terminal-host";
  const terminal = document.createElement("textarea");
  host.appendChild(terminal);
  document.body.appendChild(host);
  return { terminal, remove: () => host.remove() };
}

function press(target: EventTarget, init: KeyboardEventInit): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

beforeEach(() => window.localStorage.clear());
afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe("terminal-first preset", () => {
  it("yields Ctrl+digit, Ctrl+Tab and Ctrl+PageUp/PageDown to a focused terminal and moves tabs to Alt", () => {
    const handlers = tabHandlers();
    renderHook(() => useShortcuts(handlers as any, { isMac: false, preset: "terminal-first" }));
    const { terminal, remove } = mountTerminal();
    try {
      for (const init of [
        { key: "1", code: "Digit1", ctrlKey: true },
        { key: "Tab", code: "Tab", ctrlKey: true },
        { key: "Tab", code: "Tab", ctrlKey: true, shiftKey: true },
        { key: "PageDown", code: "PageDown", ctrlKey: true },
        { key: "PageUp", code: "PageUp", ctrlKey: true },
      ]) {
        const event = press(terminal, init);
        expect(event.defaultPrevented, JSON.stringify(init)).toBe(false);
      }
      expect(handlers["tab.select1"]).not.toHaveBeenCalled();
      expect(handlers["tab.next"]).not.toHaveBeenCalled();
      expect(handlers["tab.previous"]).not.toHaveBeenCalled();

      const alt1 = press(terminal, { key: "1", code: "Digit1", altKey: true });
      expect(alt1.defaultPrevented).toBe(true);
      expect(handlers["tab.select1"]).toHaveBeenCalledOnce();
      expect(handlers["workspace.select1"]).not.toHaveBeenCalled();

      press(terminal, { key: "PageDown", code: "PageDown", altKey: true });
      expect(handlers["tab.next"]).toHaveBeenCalledOnce();
      press(terminal, { key: "PageUp", code: "PageUp", altKey: true });
      expect(handlers["tab.previous"]).toHaveBeenCalledOnce();

      // Workspaces move to Alt+Shift+digit under the same action ids.
      press(terminal, { key: "!", code: "Digit1", altKey: true, shiftKey: true });
      expect(handlers["workspace.select1"]).toHaveBeenCalledOnce();
    } finally {
      remove();
    }
  });

  it("default preset keeps claiming Ctrl+1 and Ctrl+Tab from the terminal", () => {
    const handlers = tabHandlers();
    renderHook(() => useShortcuts(handlers as any, { isMac: false, preset: "default" }));
    const { terminal, remove } = mountTerminal();
    try {
      expect(press(terminal, { key: "1", code: "Digit1", ctrlKey: true }).defaultPrevented).toBe(true);
      expect(press(terminal, { key: "Tab", code: "Tab", ctrlKey: true }).defaultPrevented).toBe(true);
      expect(handlers["tab.select1"]).toHaveBeenCalledOnce();
      expect(handlers["tab.next"]).toHaveBeenCalledOnce();
    } finally {
      remove();
    }
  });

  it("on macOS the preset changes nothing", () => {
    expect(resolvedShortcuts({}, "terminal-first", true)).toBe(SHORTCUTS);
    expect(shortcutLabel("tab.select1", true, {}, "terminal-first")).toBe("⌃1");
    expect(shortcutLabel("workspace.select1", true, {}, "terminal-first")).toBe("⌘1");
    expect(shortcutLabel("terminal.copyMode", true, {}, "terminal-first")).toBe("⌘⇧Space");
    expect(nativeShortcutTableFor({}, "terminal-first", true)).toEqual({});

    const handlers = tabHandlers();
    renderHook(() => useShortcuts(handlers as any, { isMac: true, preset: "terminal-first" }));
    const { terminal, remove } = mountTerminal();
    try {
      expect(press(terminal, { key: "1", code: "Digit1", ctrlKey: true }).defaultPrevented).toBe(true);
      expect(handlers["tab.select1"]).toHaveBeenCalledOnce();
    } finally {
      remove();
    }
    // A Mac never gets terminal-first as its initial preset.
    expect(loadShortcutPreset(window.localStorage, true)).toBe("default");
  });

  it("existing overrides survive a preset switch and apply on top of either preset", () => {
    saveShortcutOverrides({ "tab.select2": { key: "2", mod: true, shift: true }, "terminal.splitRight": null });
    const before = window.localStorage.getItem(SHORTCUT_OVERRIDES_STORAGE_KEY);

    saveShortcutPreset("terminal-first");
    expect(window.localStorage.getItem(SHORTCUT_PRESET_STORAGE_KEY)).toBe("terminal-first");
    expect(window.localStorage.getItem(SHORTCUT_OVERRIDES_STORAGE_KEY)).toBe(before);
    const overrides = loadShortcutOverrides();
    expect(shortcutLabel("tab.select2", false, overrides, "terminal-first")).toBe("Ctrl+Shift+2");
    expect(shortcutLabel("tab.select1", false, overrides, "terminal-first")).toBe("Alt+1");
    expect(shortcutLabel("terminal.splitRight", false, overrides, "terminal-first")).toBe("");

    saveShortcutPreset("default");
    expect(window.localStorage.getItem(SHORTCUT_OVERRIDES_STORAGE_KEY)).toBe(before);
    expect(shortcutLabel("tab.select2", false, loadShortcutOverrides(), "default")).toBe("Ctrl+Shift+2");
    expect(shortcutLabel("tab.select1", false, loadShortcutOverrides(), "default")).toBe("Ctrl+1");

    // Native key sources get the preset's chords with the user's overrides winning.
    const table = nativeShortcutTableFor(overrides, "terminal-first", false);
    expect(table["tab.select1"]).toEqual({ key: "1", alt: true });
    expect(table["tab.select2"]).toEqual({ key: "2", mod: true, shift: true });
    expect(table["terminal.splitRight"]).toBeNull();
  });

  it("new non-Mac installs start on terminal-first; existing users keep default", () => {
    expect(loadShortcutPreset(window.localStorage, false)).toBe("terminal-first");
    // The first resolution is persisted, so it does not flip once the install gains state.
    expect(window.localStorage.getItem(SHORTCUT_PRESET_STORAGE_KEY)).toBe("terminal-first");

    window.localStorage.clear();
    window.localStorage.setItem(SHORTCUT_OVERRIDES_STORAGE_KEY, JSON.stringify({ "tab.close": { key: "e", mod: true } }));
    expect(loadShortcutPreset(window.localStorage, false)).toBe("default");

    window.localStorage.clear();
    window.localStorage.setItem(PROJECTS_STORAGE_KEY, "[]");
    expect(loadShortcutPreset(window.localStorage, false)).toBe("default");

    window.localStorage.clear();
    window.localStorage.setItem(SHORTCUT_PRESET_STORAGE_KEY, "terminal-first");
    window.localStorage.setItem(PROJECTS_STORAGE_KEY, "[]");
    expect(loadShortcutPreset(window.localStorage, false)).toBe("terminal-first");
  });
});
