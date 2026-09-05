import { fireEvent, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  SHORTCUTS,
  shortcutAliasesLabels,
  shortcutLabel,
  useShortcuts,
  type ShortcutActionId,
  type ShortcutBinding,
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
      "browser.focusAddress",
      "browser.reload",
      "browser.back",
      "browser.forward",
      "browser.find",
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

  it("dispatches only registered modifier chords and preserves ordinary terminal typing and Ctrl-C", () => {
    const handlers = {
      "tab.newTerminal": vi.fn(),
      "terminal.splitRight": vi.fn(),
      "commandPalette.open": vi.fn(),
    };
    renderHook(() => useShortcuts(handlers, { isMac: true }));
    const terminalHost = document.createElement("div");
    terminalHost.className = "terminal-host";
    const terminal = document.createElement("textarea");
    terminal.className = "terminal-input";
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

  it.each(TERMINAL_CHORD_CASES)("routes %s when terminal is focused", (_label, actionId, eventInit) => {
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

type ImeEventInit = KeyboardEventInit & { keyCode?: number };

// A physical shortcut chord as it arrives from the OS: the `code` still reflects
// the shortcut key even while the keystroke is owned by an IME.
const IME_CHORDS: readonly { readonly name: string; readonly actionId: ShortcutActionId; readonly base: ImeEventInit }[] = [
  { name: "Ctrl+K command palette", actionId: "commandPalette.open", base: { key: "k", code: "KeyK", ctrlKey: true } },
  { name: "Ctrl+1 tab select", actionId: "tab.select1", base: { key: "1", code: "Digit1", ctrlKey: true } },
];

// Each entry isolates ONE IME-ownership signal so dropping any single guard
// clause in matchesBinding fails at least one case:
//  - isComposing: spec-compliant engines, key value stays the physical letter
//  - keyCode 229: legacy WebKit, isComposing false, key value stays physical
//  - key "Process": legacy composition marker without keyCode/isComposing
//  - key "Dead": dead-key composition without keyCode/isComposing
const IME_SIGNALS: readonly { readonly label: string; readonly overrides: ImeEventInit }[] = [
  { label: "isComposing-only", overrides: { isComposing: true } },
  { label: "keyCode 229-only", overrides: { isComposing: false, keyCode: 229 } },
  { label: "key Process-only", overrides: { isComposing: false, key: "Process" } },
  { label: "key Dead-only", overrides: { isComposing: false, key: "Dead" } },
];

const IME_CASES: readonly [string, ShortcutActionId, ImeEventInit][] = IME_CHORDS.flatMap((chord) =>
  IME_SIGNALS.map(
    (signal) =>
      [`${chord.name} / ${signal.label}`, chord.actionId, { ...chord.base, ...signal.overrides }] as [
        string,
        ShortcutActionId,
        ImeEventInit,
      ],
  ),
);

describe("IME-owned keydown handling", () => {
  it.each(IME_CASES)("ignores IME-owned keydown and leaves default intact: %s", (_label, actionId, eventInit) => {
    const handler = vi.fn();
    const handlers = { [actionId]: handler } as Partial<Record<ShortcutActionId, () => void>>;
    renderHook(() => useShortcuts(handlers, { isMac: false }));

    const imeEvent = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...eventInit });
    window.dispatchEvent(imeEvent);

    expect(handler).not.toHaveBeenCalled();
    expect(imeEvent.defaultPrevented).toBe(false);
  });

  it.each(IME_CHORDS.map((chord) => [chord.name, chord.actionId, chord.base] as [string, ShortcutActionId, ImeEventInit]))(
    "still routes %s when not IME-composing",
    (_label, actionId, base) => {
      const handler = vi.fn();
      const handlers = { [actionId]: handler } as Partial<Record<ShortcutActionId, () => void>>;
      renderHook(() => useShortcuts(handlers, { isMac: false }));

      const realEvent = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...base });
      window.dispatchEvent(realEvent);

      expect(handler).toHaveBeenCalledOnce();
      expect(realEvent.defaultPrevented).toBe(true);
    },
  );
});

describe("AltGr-owned keydown handling", () => {
  it("ignores an AltGr keydown that collides with the Ctrl+Alt unsplit chord and leaves default intact", () => {
    const handler = vi.fn();
    renderHook(() => useShortcuts({ "terminal.unsplit": handler }, { isMac: false }));

    // On Windows/Linux, AltGr reports ctrlKey+altKey while producing a typed
    // glyph; getModifierState("AltGraph") is the only signal distinguishing it
    // from a real Ctrl+Alt chord. Window-capture would otherwise eat the glyph.
    const altGrEvent = new KeyboardEvent("keydown", {
      key: "\u0111",
      code: "KeyD",
      ctrlKey: true,
      altKey: true,
      modifierAltGraph: true,
      bubbles: true,
      cancelable: true,
    });
    window.dispatchEvent(altGrEvent);

    expect(handler).not.toHaveBeenCalled();
    expect(altGrEvent.defaultPrevented).toBe(false);
  });

  it("still unsplits on a real Ctrl+Alt+D chord when AltGraph is not active", () => {
    const handler = vi.fn();
    renderHook(() => useShortcuts({ "terminal.unsplit": handler }, { isMac: false }));

    const chordEvent = new KeyboardEvent("keydown", {
      key: "d",
      code: "KeyD",
      ctrlKey: true,
      altKey: true,
      modifierAltGraph: false,
      bubbles: true,
      cancelable: true,
    });
    window.dispatchEvent(chordEvent);

    expect(handler).toHaveBeenCalledOnce();
    expect(chordEvent.defaultPrevented).toBe(true);
  });
});

function canonicalChordKey(binding: ShortcutBinding): string {
  const parts: string[] = [];
  if (binding.mod) parts.push("Mod");
  if (binding.control) parts.push("Control");
  if (binding.alt) parts.push("Alt");
  if (binding.shift) parts.push("Shift");
  parts.push(binding.key);
  return parts.join("+");
}

function collectShortcutCollisions(): Record<string, string[]> {
  const map: Record<string, string[]> = {};
  for (const shortcut of SHORTCUTS) {
    const chord = canonicalChordKey(shortcut.binding);
    if (!map[chord]) {
      map[chord] = [];
    }
    map[chord].push(shortcut.id);
  }
  const collisions: Record<string, string[]> = {};
  for (const [chord, ids] of Object.entries(map)) {
    if (ids.length > 1) {
      collisions[chord] = [...ids].sort();
    }
  }
  return collisions;
}

describe("shortcut chord collision invariants", () => {
  it("collision inventory is exactly the known colliding pairs", () => {
    const collisions = collectShortcutCollisions();
    expect(collisions).toEqual({
      "Mod+[": ["browser.back", "terminal.focusPrevious"],
      "Mod+]": ["browser.forward", "terminal.focusNext"],
      "Mod+f": ["browser.find", "terminal.search"],
    });
  });

  it("every colliding pair spans the browser/terminal surface split", () => {
    const collisions = collectShortcutCollisions();
    for (const [chord, ids] of Object.entries(collisions)) {
      // App.tsx guarantees only one surface's handlers are registered at a time,
      // so a collision is safe only if the two ids belong to different surfaces.
      // A collision between two same-surface ids would be a real, unresolvable conflict.
      expect(ids).toHaveLength(2);
      const hasBrowser = ids.some((id) => id.startsWith("browser."));
      const hasTerminal = ids.some((id) => id.startsWith("terminal."));
      expect(hasBrowser, `Expected chord ${chord} to have a browser action`).toBe(true);
      expect(hasTerminal, `Expected chord ${chord} to have a terminal action`).toBe(true);
    }
  });

  it("dispatcher resolves each chord by which surface's handler is registered", () => {
    // 1. With ONLY browser.back registered (terminal handler absent), dispatching Mod+[ calls browser handler
    const browserBack1 = vi.fn();
    const terminalPrev1 = vi.fn();
    const { unmount: unmount1 } = renderHook(() =>
      useShortcuts({ "browser.back": browserBack1, "terminal.focusPrevious": undefined }, { isMac: true }),
    );
    const ev1 = new KeyboardEvent("keydown", { key: "[", code: "BracketLeft", metaKey: true, bubbles: true, cancelable: true });
    window.dispatchEvent(ev1);
    expect(ev1.defaultPrevented).toBe(true);
    expect(browserBack1).toHaveBeenCalledOnce();
    expect(terminalPrev1).not.toHaveBeenCalled();
    unmount1();

    // 2. With ONLY terminal.focusPrevious registered (browser handler absent), dispatching Mod+[ calls terminal handler
    const browserBack2 = vi.fn();
    const terminalPrev2 = vi.fn();
    const { unmount: unmount2 } = renderHook(() =>
      useShortcuts({ "browser.back": undefined, "terminal.focusPrevious": terminalPrev2 }, { isMac: true }),
    );
    const ev2 = new KeyboardEvent("keydown", { key: "[", code: "BracketLeft", metaKey: true, bubbles: true, cancelable: true });
    window.dispatchEvent(ev2);
    expect(ev2.defaultPrevented).toBe(true);
    expect(terminalPrev2).toHaveBeenCalledOnce();
    expect(browserBack2).not.toHaveBeenCalled();
    unmount2();

    // 3. With ONLY browser.forward registered (terminal handler absent), dispatching Mod+] calls browser handler
    const browserForward1 = vi.fn();
    const terminalNext1 = vi.fn();
    const { unmount: unmount3 } = renderHook(() =>
      useShortcuts({ "browser.forward": browserForward1, "terminal.focusNext": undefined }, { isMac: true }),
    );
    const ev3 = new KeyboardEvent("keydown", { key: "]", code: "BracketRight", metaKey: true, bubbles: true, cancelable: true });
    window.dispatchEvent(ev3);
    expect(ev3.defaultPrevented).toBe(true);
    expect(browserForward1).toHaveBeenCalledOnce();
    expect(terminalNext1).not.toHaveBeenCalled();
    unmount3();

    // 4. With ONLY terminal.focusNext registered (browser handler absent), dispatching Mod+] calls terminal handler
    const browserForward2 = vi.fn();
    const terminalNext2 = vi.fn();
    const { unmount: unmount4 } = renderHook(() =>
      useShortcuts({ "browser.forward": undefined, "terminal.focusNext": terminalNext2 }, { isMac: true }),
    );
    const ev4 = new KeyboardEvent("keydown", { key: "]", code: "BracketRight", metaKey: true, bubbles: true, cancelable: true });
    window.dispatchEvent(ev4);
    expect(ev4.defaultPrevented).toBe(true);
    expect(terminalNext2).toHaveBeenCalledOnce();
    expect(browserForward2).not.toHaveBeenCalled();
    unmount4();
  });

  it("documents precedence order when both colliding handlers are registered (array order wins)", () => {
    // App.tsx must keep these mutually exclusive so this path is never taken in production.
    const browserBack = vi.fn();
    const terminalPrev = vi.fn();
    const { unmount } = renderHook(() =>
      useShortcuts(
        {
          "browser.back": browserBack,
          "terminal.focusPrevious": terminalPrev,
        },
        { isMac: true },
      ),
    );

    const ev = new KeyboardEvent("keydown", { key: "[", code: "BracketLeft", metaKey: true, bubbles: true, cancelable: true });
    window.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
    expect(browserBack).toHaveBeenCalledOnce();
    expect(terminalPrev).not.toHaveBeenCalled();
    unmount();
  });
});
