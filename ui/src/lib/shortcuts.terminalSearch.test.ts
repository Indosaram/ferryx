import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { findConflict, shortcutAliasesLabels, shortcutLabel, useShortcuts } from "./shortcuts";

function terminalTarget() {
  const host = document.createElement("div");
  host.className = "terminal-host";
  const sink = document.createElement("textarea");
  host.append(sink);
  document.body.append(host);
  sink.focus();
  return sink;
}

function press(target: EventTarget, init: KeyboardEventInit) {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

afterEach(() => {
  cleanup();
  document.querySelectorAll(".terminal-host").forEach((host) => host.remove());
});

for (const preset of ["default", "terminal-first"] as const) {
  describe(`terminal search (${preset})`, () => {
    it("opens on Ctrl+Shift+F while leaving bare Ctrl+F to the focused shell", () => {
      const search = vi.fn();
      renderHook(() => useShortcuts({ "terminal.search": search }, { isMac: false, preset }));
      const sink = terminalTarget();
      expect(press(sink, { key: "f", code: "KeyF", ctrlKey: true }).defaultPrevented).toBe(false);
      expect(search).not.toHaveBeenCalled();
      expect(press(sink, { key: "F", code: "KeyF", ctrlKey: true, shiftKey: true }).defaultPrevented).toBe(true);
      expect(search).toHaveBeenCalledOnce();
      expect(shortcutLabel("terminal.search", false, {}, preset)).toBe("Ctrl+Shift+F");
    });

    it("keeps the old Ctrl+F alias outside the terminal", () => {
      const search = vi.fn();
      renderHook(() => useShortcuts({ "terminal.search": search }, { isMac: false, preset }));
      expect(press(document.body, { key: "f", code: "KeyF", ctrlKey: true }).defaultPrevented).toBe(true);
      expect(search).toHaveBeenCalledOnce();
      expect(shortcutAliasesLabels("terminal.search", false, {}, preset)).toEqual(["Ctrl+F"]);
    });

    it("preserves macOS Cmd+F without claiming readline Ctrl+F or Cmd+Shift+F", () => {
      const search = vi.fn();
      renderHook(() => useShortcuts({ "terminal.search": search }, { isMac: true, preset }));
      const sink = terminalTarget();
      expect(press(sink, { key: "f", code: "KeyF", ctrlKey: true }).defaultPrevented).toBe(false);
      expect(press(sink, { key: "F", code: "KeyF", metaKey: true, shiftKey: true }).defaultPrevented).toBe(false);
      expect(search).not.toHaveBeenCalled();
      expect(press(sink, { key: "f", code: "KeyF", metaKey: true }).defaultPrevented).toBe(true);
      expect(search).toHaveBeenCalledOnce();
      expect(shortcutLabel("terminal.search", true, {}, preset)).toBe("⌘F");
      expect(shortcutAliasesLabels("terminal.search", true, {}, preset)).toEqual([]);
    });

    it("lets an explicit override replace both the primary binding and compatibility alias", () => {
      const search = vi.fn();
      const overrides = { "terminal.search": { key: "g", mod: true, shift: true } };
      renderHook(() => useShortcuts({ "terminal.search": search }, { isMac: false, preset, overrides }));
      const sink = terminalTarget();
      expect(press(sink, { key: "F", code: "KeyF", ctrlKey: true, shiftKey: true }).defaultPrevented).toBe(false);
      expect(press(document.body, { key: "f", code: "KeyF", ctrlKey: true }).defaultPrevented).toBe(false);
      expect(search).not.toHaveBeenCalled();
      expect(press(sink, { key: "G", code: "KeyG", ctrlKey: true, shiftKey: true }).defaultPrevented).toBe(true);
      expect(search).toHaveBeenCalledOnce();
      expect(shortcutAliasesLabels("terminal.search", false, overrides, preset)).toEqual([]);
    });

    it("respects an explicitly unbound search action", () => {
      const search = vi.fn();
      renderHook(() => useShortcuts({ "terminal.search": search }, {
        isMac: false, preset, overrides: { "terminal.search": null },
      }));
      expect(press(terminalTarget(), { key: "F", code: "KeyF", ctrlKey: true, shiftKey: true }).defaultPrevented).toBe(false);
      expect(press(document.body, { key: "f", code: "KeyF", ctrlKey: true }).defaultPrevented).toBe(false);
      expect(search).not.toHaveBeenCalled();
    });

    it("has no conflict and reserves its new chord against other action overrides", () => {
      const chord = { key: "f", mod: true, shift: true };
      expect(findConflict("terminal.search", chord, {}, false, preset)).toBeNull();
      expect(findConflict("tab.newTerminal", chord, {}, false, preset)).toMatchObject({
        kind: "action", actionId: "terminal.search",
      });
    });
  });
}
