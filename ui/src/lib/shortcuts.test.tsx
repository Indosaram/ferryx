import { fireEvent, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  SHORTCUTS,
  shortcutAliasesLabels,
  shortcutLabel,
  useShortcuts,
  type ShortcutActionId,
} from "./shortcuts";

const TERMINAL_CHORD_CASES: readonly [string, ShortcutActionId, KeyboardEventInit][] = [
  ["new terminal tab", "tab.newTerminal", { key: "t", metaKey: true }],
  ["new browser tab", "tab.newBrowser", { key: "b", metaKey: true, shiftKey: true }],
  ["close active tab", "tab.close", { key: "w", metaKey: true }],
  ["next terminal tab primary", "tab.next", { key: "]", code: "BracketRight", metaKey: true, shiftKey: true }],
  ["next terminal tab alias page-down", "tab.next", { key: "PageDown", ctrlKey: true }],
  ["next terminal tab alias ctrl-tab", "tab.next", { key: "Tab", code: "Tab", ctrlKey: true }],
  ["previous terminal tab primary", "tab.previous", { key: "[", code: "BracketLeft", metaKey: true, shiftKey: true }],
  ["previous terminal tab alias page-up", "tab.previous", { key: "PageUp", ctrlKey: true }],
  ["previous terminal tab alias ctrl-shift-tab", "tab.previous", { key: "Tab", code: "Tab", ctrlKey: true, shiftKey: true }],
  ["split terminal right", "terminal.splitRight", { key: "d", metaKey: true }],
  ["split terminal down", "terminal.splitDown", { key: "d", metaKey: true, shiftKey: true }],
  ["close split view", "terminal.unsplit", { key: "d", metaKey: true, altKey: true }],
  ["close split view alias cmd-shift-w", "terminal.unsplit", { key: "w", metaKey: true, shiftKey: true }],
  [
    "close split view with the macOS Option glyph",
    "terminal.unsplit",
    { key: "∂", code: "KeyD", metaKey: true, altKey: true },
  ],
  ["focus next terminal pane", "terminal.focusNext", { key: "]", metaKey: true }],
  ["focus previous terminal pane", "terminal.focusPrevious", { key: "[", metaKey: true }],
  ["find in terminal", "terminal.search", { key: "f", metaKey: true }],
  ["toggle sidebar", "sidebar.left.toggle", { key: "b", metaKey: true }],
  ["open command palette primary", "commandPalette.open", { key: "k", metaKey: true }],
  ["open command palette alias cmd-p", "commandPalette.open", { key: "p", metaKey: true }],
  ["toggle settings", "settings.toggle", { key: ",", metaKey: true }],
  ["zoom in primary", "zoom.in", { key: "=", code: "Equal", metaKey: true }],
  ["zoom in alias shift-plus", "zoom.in", { key: "+", code: "Equal", metaKey: true, shiftKey: true }],
  ["zoom out", "zoom.out", { key: "-", code: "Minus", metaKey: true }],
  ["zoom reset", "zoom.reset", { key: "0", code: "Digit0", metaKey: true }],
  ["select workspace 1", "workspace.select1", { key: "1", metaKey: true }],
  ["select workspace 2", "workspace.select2", { key: "2", metaKey: true }],
  ["select workspace 3", "workspace.select3", { key: "3", metaKey: true }],
  ["select workspace 4", "workspace.select4", { key: "4", metaKey: true }],
  ["select workspace 5", "workspace.select5", { key: "5", metaKey: true }],
  ["select workspace 6", "workspace.select6", { key: "6", metaKey: true }],
  ["select workspace 7", "workspace.select7", { key: "7", metaKey: true }],
  ["select workspace 8", "workspace.select8", { key: "8", metaKey: true }],
  ["select workspace 9", "workspace.select9", { key: "9", metaKey: true }],
  ["select terminal tab 1", "tab.select1", { key: "1", ctrlKey: true }],
  ["select terminal tab 2", "tab.select2", { key: "2", ctrlKey: true }],
  ["select terminal tab 3", "tab.select3", { key: "3", ctrlKey: true }],
  ["select terminal tab 4", "tab.select4", { key: "4", ctrlKey: true }],
  ["select terminal tab 5", "tab.select5", { key: "5", ctrlKey: true }],
  ["select terminal tab 6", "tab.select6", { key: "6", ctrlKey: true }],
  ["select terminal tab 7", "tab.select7", { key: "7", ctrlKey: true }],
  ["select terminal tab 8", "tab.select8", { key: "8", ctrlKey: true }],
  ["select terminal tab 9", "tab.select9", { key: "9", ctrlKey: true }],
];

describe("shortcut registry", () => {
  it("contains the required frontend actions with platform-aware Mod labels and aliases", () => {
    expect(SHORTCUTS.map((shortcut) => shortcut.id)).toEqual([
      "tab.newTerminal",
      "tab.newBrowser",
      "tab.close",
      "tab.next",
      "tab.previous",
      "tab.select1",
      "tab.select2",
      "tab.select3",
      "tab.select4",
      "tab.select5",
      "tab.select6",
      "tab.select7",
      "tab.select8",
      "tab.select9",
      "workspace.select1",
      "workspace.select2",
      "workspace.select3",
      "workspace.select4",
      "workspace.select5",
      "workspace.select6",
      "workspace.select7",
      "workspace.select8",
      "workspace.select9",
      "terminal.splitRight",
      "terminal.splitDown",
      "terminal.unsplit",
      "terminal.focusNext",
      "terminal.focusPrevious",
      "terminal.search",
      "sidebar.left.toggle",
      "commandPalette.open",
      "settings.toggle",
      "zoom.in",
      "zoom.out",
      "zoom.reset",
    ]);

    expect(shortcutLabel("tab.newTerminal", true)).toBe("⌘T");
    expect(shortcutLabel("tab.newTerminal", false)).toBe("Ctrl+T");
    expect(shortcutLabel("sidebar.left.toggle", true)).toBe("⌘B");
    expect(shortcutLabel("sidebar.left.toggle", false)).toBe("Ctrl+B");
    expect(shortcutLabel("terminal.splitDown", true)).toBe("⌘⇧D");
    expect(shortcutLabel("terminal.focusNext", true)).toBe("⌘]");
    expect(shortcutLabel("terminal.focusNext", false)).toBe("Ctrl+]");
    expect(shortcutLabel("terminal.focusPrevious", true)).toBe("⌘[");
    expect(shortcutLabel("terminal.focusPrevious", false)).toBe("Ctrl+[");
    expect(shortcutLabel("tab.next", true)).toBe("⌘⇧]");
    expect(shortcutAliasesLabels("tab.next", true)).toEqual(["⌃PgDn", "⌃Tab"]);
    expect(shortcutAliasesLabels("tab.next", false)).toEqual(["Ctrl+PgDn", "Ctrl+Tab"]);
    expect(shortcutLabel("tab.previous", true)).toBe("⌘⇧[");
    expect(shortcutAliasesLabels("tab.previous", true)).toEqual(["⌃PgUp", "⌃⇧Tab"]);
    expect(shortcutLabel("terminal.search", true)).toBe("⌘F");
    expect(shortcutLabel("terminal.search", false)).toBe("Ctrl+F");
    expect(shortcutLabel("settings.toggle", true)).toBe("⌘,");
    expect(shortcutLabel("settings.toggle", false)).toBe("Ctrl+,");
    expect(shortcutLabel("workspace.select1", true)).toBe("⌘1");
    expect(shortcutLabel("workspace.select1", false)).toBe("Ctrl+1");
    expect(shortcutLabel("workspace.select9", true)).toBe("⌘9");
    expect(shortcutLabel("tab.select1", true)).toBe("⌃1");
    expect(shortcutLabel("tab.select1", false)).toBe("Ctrl+1");
    expect(shortcutLabel("tab.select9", true)).toBe("⌃9");
    expect(shortcutLabel("zoom.in", true)).toBe("⌘=");
    expect(shortcutLabel("zoom.out", true)).toBe("⌘-");
    expect(shortcutLabel("zoom.reset", true)).toBe("⌘0");
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
    const handlers: Partial<Record<ShortcutActionId, ReturnType<typeof vi.fn>>> = {
      "tab.newTerminal": vi.fn(),
      "tab.newBrowser": vi.fn(),
      "tab.close": vi.fn(),
      "tab.next": vi.fn(),
      "tab.previous": vi.fn(),
      "tab.select1": vi.fn(),
      "tab.select2": vi.fn(),
      "tab.select3": vi.fn(),
      "tab.select4": vi.fn(),
      "tab.select5": vi.fn(),
      "tab.select6": vi.fn(),
      "tab.select7": vi.fn(),
      "tab.select8": vi.fn(),
      "tab.select9": vi.fn(),
      "workspace.select1": vi.fn(),
      "workspace.select2": vi.fn(),
      "workspace.select3": vi.fn(),
      "workspace.select4": vi.fn(),
      "workspace.select5": vi.fn(),
      "workspace.select6": vi.fn(),
      "workspace.select7": vi.fn(),
      "workspace.select8": vi.fn(),
      "workspace.select9": vi.fn(),
      "terminal.splitRight": vi.fn(),
      "terminal.splitDown": vi.fn(),
      "terminal.unsplit": vi.fn(),
      "terminal.focusNext": vi.fn(),
      "terminal.focusPrevious": vi.fn(),
      "terminal.search": vi.fn(),
      "sidebar.left.toggle": vi.fn(),
      "commandPalette.open": vi.fn(),
      "settings.toggle": vi.fn(),
      "zoom.in": vi.fn(),
      "zoom.out": vi.fn(),
      "zoom.reset": vi.fn(),
    };
    renderHook(() => useShortcuts(handlers as any, { isMac: true }));
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
      "tab.close": vi.fn(),
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
