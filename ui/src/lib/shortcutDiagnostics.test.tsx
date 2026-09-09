import { renderHook } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { useShortcuts } from "./shortcuts";
import { installShortcutDiagnostics, traceShortcutAction } from "./shortcutDiagnostics";
import { switchDebug } from "./switchDebug";
import { onNewTerminalTabMenu } from "./tauri";

vi.mock("./switchDebug", () => ({ switchDebug: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: vi.fn() }));
const native = vi.hoisted(() => ({ callback: undefined as undefined | ((event: { payload: undefined }) => unknown), dispose: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (_name, callback) => {
  native.callback = callback;
  return native.dispose;
}) }));
beforeEach(() => vi.clearAllMocks());

it("traces registration, receipt, match, invocation, return and disposal without changing dispatch", () => {
  const stop = installShortcutDiagnostics();
  const handler = vi.fn();
  const { unmount } = renderHook(() => useShortcuts({ "tab.newTerminal": handler }, { isMac: true }));
  const event = new KeyboardEvent("keydown", { key: "t", code: "KeyT", metaKey: true, cancelable: true });
  window.dispatchEvent(event);
  expect(handler).toHaveBeenCalledOnce();
  expect(event.defaultPrevented).toBe(true);
  unmount();
  stop();
  expect(vi.mocked(switchDebug).mock.calls.map(([name]) => name)).toEqual(expect.arrayContaining([
    "shortcut.hook.register", "shortcut.webview.keydown", "shortcut.hook.receipt",
    "shortcut.hook.match", "shortcut.action.invoke", "shortcut.action.return", "shortcut.hook.unregister",
  ]));
});

it("records disabled, mismatch, IME and editable rejection but never ordinary typing", () => {
  const handler = vi.fn();
  const { unmount } = renderHook(() => useShortcuts({ "tab.newTerminal": handler }, { isMac: true }));
  const input = document.createElement("input");
  input.value = "private text";
  document.body.append(input);
  input.focus();
  vi.mocked(switchDebug).mockClear();
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "x", bubbles: true }));
  expect(switchDebug).not.toHaveBeenCalled();
  for (const init of [{ key: "t" }, { key: "w" }, { key: "t", isComposing: true }]) {
    const event = new KeyboardEvent("keydown", { ...init, metaKey: true, bubbles: true, cancelable: true });
    input.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  }
  expect(handler).not.toHaveBeenCalled();
  const reasons = vi.mocked(switchDebug).mock.calls.filter(([name]) => name === "shortcut.hook.reject").map(([, details]) => details?.reason);
  expect(reasons).toEqual(expect.arrayContaining(["handler-disabled", "binding-mismatch", "ime", "editable-target"]));
  expect(JSON.stringify(vi.mocked(switchDebug).mock.calls)).not.toContain("private text");
  input.remove();
  unmount();
});

it("preserves exact synchronous exceptions and return values, including untouched promises", async () => {
  const error = new Error("private error details");
  expect(() => traceShortcutAction("test", () => { throw error; })).toThrow(error);
  expect(switchDebug).toHaveBeenCalledWith("shortcut.action.error", expect.objectContaining({ action: "test", errorType: "Error" }));
  expect(traceShortcutAction("test", () => 42)).toBe(42);
  const promise = Promise.reject(error);
  const observed = expect(promise).rejects.toBe(error);
  expect(traceShortcutAction("test", () => promise)).toBe(promise);
  await observed;
  expect(JSON.stringify(vi.mocked(switchDebug).mock.calls)).not.toContain("private error details");
});

it("traces native menu callbacks and preserves thrown errors and disposal", async () => {
  const error = new Error("failure");
  const handler = vi.fn(() => { throw error; });
  const stop = await onNewTerminalTabMenu(handler);
  expect(() => native.callback!({ payload: undefined })).toThrow(error);
  expect(handler).toHaveBeenCalledOnce();
  expect(switchDebug).toHaveBeenCalledWith("shortcut.native.receipt", expect.objectContaining({ action: "menu_new_terminal_tab" }));
  stop();
  expect(native.dispose).toHaveBeenCalledOnce();
});
