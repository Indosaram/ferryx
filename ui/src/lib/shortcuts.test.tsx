import { fireEvent, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { SHORTCUTS, shortcutLabel, useShortcuts } from "./shortcuts";

describe("shortcut registry", () => {
  it("contains the required frontend actions with platform-aware Mod labels", () => {
    expect(SHORTCUTS.map((shortcut) => shortcut.id)).toEqual([
      "tab.newTerminal",
      "tab.close",
      "tab.next",
      "tab.previous",
      "terminal.splitRight",
      "terminal.splitDown",
      "terminal.unsplit",
      "commandPalette.open",
    ]);
    expect(shortcutLabel("tab.newTerminal", true)).toBe("⌘T");
    expect(shortcutLabel("tab.newTerminal", false)).toBe("Ctrl+T");
    expect(shortcutLabel("terminal.splitDown", true)).toBe("⌘⇧D");
  });

  it("dispatches matching shortcuts and prevents the browser default", () => {
    const handlers = {
      "tab.newTerminal": vi.fn(),
      "terminal.splitRight": vi.fn(),
      "commandPalette.open": vi.fn(),
    };
    renderHook(() => useShortcuts(handlers, { isMac: true }));

    const newTab = new KeyboardEvent("keydown", { key: "t", metaKey: true, cancelable: true });
    window.dispatchEvent(newTab);
    expect(handlers["tab.newTerminal"]).toHaveBeenCalledOnce();
    expect(newTab.defaultPrevented).toBe(true);

    fireEvent.keyDown(window, { key: "d", metaKey: true });
    expect(handlers["terminal.splitRight"]).toHaveBeenCalledOnce();

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    expect(handlers["commandPalette.open"]).toHaveBeenCalledOnce();
  });

  it("does not steal typing shortcuts from editable fields except command palette", () => {
    const handlers = {
      "tab.newTerminal": vi.fn(),
      "commandPalette.open": vi.fn(),
    };
    renderHook(() => useShortcuts(handlers, { isMac: false }));
    const input = document.createElement("input");
    document.body.appendChild(input);

    fireEvent.keyDown(input, { key: "t", ctrlKey: true });
    expect(handlers["tab.newTerminal"]).not.toHaveBeenCalled();

    fireEvent.keyDown(input, { key: "k", ctrlKey: true });
    expect(handlers["commandPalette.open"]).toHaveBeenCalledOnce();
    input.remove();
  });
});
