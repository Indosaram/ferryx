import { act, cleanup, render, renderHook } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CommandPalette } from "../components/CommandPalette";
import { FeaturesStep } from "../components/onboarding/FeaturesStep";
import {
  loadShortcutOverrides,
  saveShortcutOverrides,
  useShortcutOverrides,
} from "./shortcutOverrides";
import {
  findConflict,
  resolvedShortcuts,
  shortcutAliasesLabels,
  shortcutLabel,
  useShortcuts,
} from "./shortcuts";
import { SHORTCUT_OVERRIDES_STORAGE_KEY } from "./storageKeys";

beforeEach(() => window.localStorage.clear());
afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe("shortcut overrides", () => {
  it("override replaces the primary binding everywhere labels are shown", () => {
    const overrides = saveShortcutOverrides({ "commandPalette.open": { key: "j", mod: true, shift: true } });

    expect(shortcutLabel("commandPalette.open", true, overrides)).toBe("⌘⇧J");
    expect(shortcutLabel("commandPalette.open", false, overrides)).toBe("Ctrl+Shift+J");
    // The override drops the default Cmd+P alias along with the primary binding.
    expect(shortcutAliasesLabels("commandPalette.open", true, overrides)).toEqual([]);
    expect(resolvedShortcuts(overrides).find((shortcut) => shortcut.id === "commandPalette.open")?.binding)
      .toEqual({ key: "j", mod: true, shift: true });

    const palette = render(createElement(CommandPalette, {
      open: true, worktrees: [], tabs: [], onSelectWorktree: () => undefined, onSelectTab: () => undefined, onClose: () => undefined,
    }));
    const paletteLabels = Array.from(palette.container.ownerDocument.querySelectorAll("kbd")).map((kbd) => kbd.textContent);
    expect(paletteLabels).toContain(shortcutLabel("commandPalette.open", undefined, overrides));
    expect(paletteLabels).not.toContain(shortcutLabel("commandPalette.open"));
    palette.unmount();

    const features = render(createElement(FeaturesStep, { isMac: true }));
    expect(features.container.textContent).toContain("⌘⇧J");
    expect(features.container.textContent).not.toContain("⌘K");

    // The dispatcher follows the same table: the new chord fires, the old primary and alias do not.
    const open = vi.fn();
    renderHook(() => useShortcuts({ "commandPalette.open": open }, { isMac: true, overrides }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, cancelable: true }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "p", metaKey: true, cancelable: true }));
    expect(open).not.toHaveBeenCalled();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "j", metaKey: true, shiftKey: true, cancelable: true }));
    expect(open).toHaveBeenCalledOnce();
  });

  it("null override unbinds and releases the chord to the terminal", () => {
    const overrides = saveShortcutOverrides({ "terminal.splitRight": null });
    expect(loadShortcutOverrides()).toEqual({ "terminal.splitRight": null });
    expect(resolvedShortcuts(overrides).some((shortcut) => shortcut.id === "terminal.splitRight")).toBe(false);
    expect(shortcutLabel("terminal.splitRight", true, overrides)).toBe("");

    const split = vi.fn();
    renderHook(() => useShortcuts({ "terminal.splitRight": split }, { isMac: true, overrides }));
    const terminalHost = document.createElement("div");
    terminalHost.className = "terminal-host";
    const terminal = document.createElement("textarea");
    terminalHost.appendChild(terminal);
    document.body.appendChild(terminalHost);
    const cmdD = new KeyboardEvent("keydown", { key: "d", code: "KeyD", metaKey: true, cancelable: true, bubbles: true });
    terminal.dispatchEvent(cmdD);
    expect(split).not.toHaveBeenCalled();
    expect(cmdD.defaultPrevented).toBe(false);
    terminalHost.remove();

    // The released chord is free to bind to another action.
    expect(findConflict("terminal.splitDown", { key: "d", mod: true }, overrides, true)).toBeNull();
  });

  it("findConflict names the owning action", () => {
    expect(findConflict("tab.newTerminal", { key: "b", mod: true }, {}, true))
      .toEqual({ kind: "action", actionId: "sidebar.left.toggle", title: "Toggle sidebar" });
    // Aliases own their chords too.
    expect(findConflict("tab.newTerminal", { key: "p", mod: true }, {}, true))
      .toMatchObject({ kind: "action", actionId: "commandPalette.open" });
    // An override moves ownership: the old chord is free and the new one is owned.
    const overrides = { "sidebar.left.toggle": { key: "y", mod: true } };
    expect(findConflict("tab.newTerminal", { key: "b", mod: true }, overrides, true)).toBeNull();
    expect(findConflict("tab.newTerminal", { key: "y", mod: true }, overrides, true))
      .toMatchObject({ kind: "action", actionId: "sidebar.left.toggle" });
    // Rebinding an action to its own chord is not a conflict.
    expect(findConflict("sidebar.left.toggle", { key: "b", mod: true }, {}, true)).toBeNull();
    // Off macOS mod resolves to Ctrl, so Ctrl+B collides with the abstract Mod+B owner.
    expect(findConflict("tab.newTerminal", { key: "b", control: true }, {}, false))
      .toMatchObject({ kind: "action", actionId: "sidebar.left.toggle" });
  });

  it("rejects binding to a reserved clipboard/menu chord", () => {
    for (const key of ["c", "v", "q", "h", "m"]) {
      expect(findConflict("tab.newTerminal", { key, mod: true }, {}, true), `Cmd+${key}`).toMatchObject({ kind: "reserved" });
    }
    expect(findConflict("tab.newTerminal", { key: "c", mod: true, shift: true }, {}, false))
      .toMatchObject({ kind: "reserved", chord: { id: "clipboard.copyTerminal" } });
    expect(findConflict("tab.newTerminal", { key: "v", mod: true, shift: true }, {}, false))
      .toMatchObject({ kind: "reserved", chord: { id: "clipboard.pasteTerminalShift" } });
    // Cmd+Q/H/M belong to the macOS app menu only; off macOS Ctrl+Q is not reserved.
    expect(findConflict("tab.newTerminal", { key: "q", mod: true }, {}, false)).toBeNull();
  });

  it("malformed stored overrides normalize to defaults", () => {
    window.localStorage.setItem(SHORTCUT_OVERRIDES_STORAGE_KEY, "{not json");
    expect(loadShortcutOverrides()).toEqual({});

    window.localStorage.setItem(SHORTCUT_OVERRIDES_STORAGE_KEY, JSON.stringify([{ key: "x" }]));
    expect(loadShortcutOverrides()).toEqual({});

    window.localStorage.setItem(SHORTCUT_OVERRIDES_STORAGE_KEY, JSON.stringify({
      "unknown.action": { key: "x", mod: true },
      "tab.close": { key: 42 },
      "tab.next": { key: "", mod: true },
      "tab.previous": { key: "x", mod: "yes" },
      "zoom.in": "Cmd+=",
      "tab.newTerminal": { key: "y", mod: true, nonMac: { alt: true }, extra: 1 },
    }));
    const overrides = loadShortcutOverrides();
    expect(overrides).toEqual({ "tab.newTerminal": { key: "y", mod: true } });
    expect(shortcutLabel("tab.close", true, overrides)).toBe("⌘W");
    expect(shortcutAliasesLabels("tab.next", true, overrides)).toEqual(["⌃PgDn", "⌃Tab"]);
  });

  it("storage event from another window applies live", () => {
    const { result } = renderHook(() => useShortcutOverrides());
    expect(result.current.overrides).toEqual({});

    // Another window writes storage directly; only the storage event reaches this one.
    const value = JSON.stringify({ "tab.close": { key: "e", mod: true } });
    window.localStorage.setItem(SHORTCUT_OVERRIDES_STORAGE_KEY, value);
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: SHORTCUT_OVERRIDES_STORAGE_KEY, newValue: value }));
    });
    expect(result.current.overrides).toEqual({ "tab.close": { key: "e", mod: true } });
    expect(shortcutLabel("tab.close", true, result.current.overrides)).toBe("⌘E");

    window.localStorage.removeItem(SHORTCUT_OVERRIDES_STORAGE_KEY);
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: SHORTCUT_OVERRIDES_STORAGE_KEY, newValue: null }));
    });
    expect(result.current.overrides).toEqual({});
  });
});
