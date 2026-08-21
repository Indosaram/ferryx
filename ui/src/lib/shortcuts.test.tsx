import { fireEvent, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { SHORTCUTS, shortcutLabel, useShortcuts, type ShortcutActionId } from "./shortcuts";

const TERMINAL_CHORD_CASES: readonly [string, ShortcutActionId, KeyboardEventInit][] = [
  ["new terminal tab", "tab.newTerminal", { key: "t", metaKey: true }],
  ["close active tab", "tab.close", { key: "w", metaKey: true }],
  ["next terminal tab", "tab.next", { key: "PageDown", ctrlKey: true }],
  ["previous terminal tab", "tab.previous", { key: "PageUp", ctrlKey: true }],
  ["split terminal right", "terminal.splitRight", { key: "d", metaKey: true }],
  ["split terminal down", "terminal.splitDown", { key: "d", metaKey: true, shiftKey: true }],
  ["close split view", "terminal.unsplit", { key: "d", metaKey: true, altKey: true }],
  [
    "close split view with the macOS Option glyph",
    "terminal.unsplit",
    { key: "∂", code: "KeyD", metaKey: true, altKey: true },
  ],
  ["toggle sidebar", "sidebar.left.toggle", { key: "b", metaKey: true }],
  ["open command palette", "commandPalette.open", { key: "k", metaKey: true }],
  ["select workspace 1", "workspace.select1", { key: "1", metaKey: true }],
  ["select workspace 2", "workspace.select2", { key: "2", metaKey: true }],
  ["select workspace 3", "workspace.select3", { key: "3", metaKey: true }],
  ["select workspace 4", "workspace.select4", { key: "4", metaKey: true }],
  ["select terminal tab 1", "tab.select1", { key: "1", ctrlKey: true }],
  ["select terminal tab 2", "tab.select2", { key: "2", ctrlKey: true }],
  ["select terminal tab 3", "tab.select3", { key: "3", ctrlKey: true }],
  ["select terminal tab 4", "tab.select4", { key: "4", ctrlKey: true }],
];

describe("shortcut registry", () => {
  it("contains the required frontend actions with platform-aware Mod labels", () => {
    expect(SHORTCUTS.map((shortcut) => shortcut.id)).toEqual([
      "tab.newTerminal",
      "tab.close",
      "tab.next",
      "tab.previous",
      "tab.select1",
      "tab.select2",
      "tab.select3",
      "tab.select4",
      "workspace.select1",
      "workspace.select2",
      "workspace.select3",
      "workspace.select4",
      "terminal.splitRight",
      "terminal.splitDown",
      "terminal.unsplit",
      "sidebar.left.toggle",
      "commandPalette.open",
    ]);
    expect(shortcutLabel("tab.newTerminal", true)).toBe("⌘T");
    expect(shortcutLabel("tab.newTerminal", false)).toBe("Ctrl+T");
    expect(shortcutLabel("sidebar.left.toggle", true)).toBe("⌘B");
    expect(shortcutLabel("sidebar.left.toggle", false)).toBe("Ctrl+B");
    expect(shortcutLabel("terminal.splitDown", true)).toBe("⌘⇧D");
    expect(shortcutLabel("workspace.select1", true)).toBe("⌘1");
    expect(shortcutLabel("workspace.select1", false)).toBe("Ctrl+1");
    expect(shortcutLabel("tab.select1", true)).toBe("⌃1");
    expect(shortcutLabel("tab.select1", false)).toBe("Ctrl+1");
  });

  it("registers the app shortcut router in capture phase", () => {
    const addEventListener = vi.spyOn(window, "addEventListener");
    const { unmount } = renderHook(() => useShortcuts({ "tab.newTerminal": vi.fn() }, { isMac: true }));

    expect(addEventListener).toHaveBeenCalledWith("keydown", expect.any(Function), true);
    unmount();
    addEventListener.mockRestore();
  });

  it("dispatches only registered modifier chords and preserves ordinary xterm typing and Ctrl-C", () => {
    const handlers = {
      "tab.newTerminal": vi.fn(),
      "terminal.splitRight": vi.fn(),
      "commandPalette.open": vi.fn(),
    };
    renderHook(() => useShortcuts(handlers, { isMac: true }));
    const terminalHost = document.createElement("div");
    terminalHost.className = "terminal-host";
    const terminal = document.createElement("textarea");
    terminal.className = "xterm-helper-textarea";
    terminalHost.appendChild(terminal);
    document.body.appendChild(terminalHost);

    const ordinary = new KeyboardEvent("keydown", { key: "x", cancelable: true, bubbles: true });
    terminal.dispatchEvent(ordinary);
    expect(ordinary.defaultPrevented).toBe(false);

    const ctrlC = new KeyboardEvent("keydown", { key: "c", ctrlKey: true, cancelable: true, bubbles: true });
    terminal.dispatchEvent(ctrlC);
    expect(ctrlC.defaultPrevented).toBe(false);

    const split = new KeyboardEvent("keydown", { key: "d", metaKey: true, cancelable: true, bubbles: true });
    terminal.dispatchEvent(split);
    expect(split.defaultPrevented).toBe(true);
    expect(handlers["terminal.splitRight"]).toHaveBeenCalledOnce();

    terminalHost.remove();
  });

  it.each(TERMINAL_CHORD_CASES)("routes %s when xterm is focused", (_label, actionId, eventInit) => {
    const handlers = {
      "tab.newTerminal": vi.fn(),
      "tab.close": vi.fn(),
      "tab.next": vi.fn(),
      "tab.previous": vi.fn(),
      "tab.select1": vi.fn(),
      "tab.select2": vi.fn(),
      "tab.select3": vi.fn(),
      "tab.select4": vi.fn(),
      "workspace.select1": vi.fn(),
      "workspace.select2": vi.fn(),
      "workspace.select3": vi.fn(),
      "workspace.select4": vi.fn(),
      "terminal.splitRight": vi.fn(),
      "terminal.splitDown": vi.fn(),
      "terminal.unsplit": vi.fn(),
      "sidebar.left.toggle": vi.fn(),
      "commandPalette.open": vi.fn(),
    };
    renderHook(() => useShortcuts(handlers, { isMac: true }));
    const terminalHost = document.createElement("div");
    terminalHost.className = "terminal-host";
    const terminal = document.createElement("textarea");
    terminalHost.appendChild(terminal);
    document.body.appendChild(terminalHost);

    const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...eventInit });
    terminal.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
    expect(handlers[actionId]).toHaveBeenCalledOnce();
    terminalHost.remove();
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
