// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useScripting, dispatchScriptKey, scriptControl } from "../../lib/scripting";
import { ScriptingSection } from "./ScriptingSection";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("../../lib/tauri", () => ({ toIpcError: (error: unknown) => ({ message: String(error) }) }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

describe("Scripting", () => {
  it("persists enable, reloads, and renders the daemon log", async () => {
    invoke.mockResolvedValue({ enabled: false, bindings: [], log: "first" });
    await act(async () => { render(<ScriptingSection />); });
    expect(screen.getByRole("log").textContent).toBe("first");
    invoke.mockResolvedValue({ enabled: true, bindings: [], log: "loaded" });
    await act(async () => { fireEvent.click(screen.getByRole("checkbox")); });
    expect(invoke).toHaveBeenLastCalledWith("cmd_script", { action: { action: "setEnabled", enabled: true } });
    await act(async () => { fireEvent.click(screen.getAllByRole("button")[0]); });
    expect(invoke).toHaveBeenLastCalledWith("cmd_script", { action: { action: "reload" } });
    expect(screen.getByRole("log").textContent).toBe("loaded");
  });

  it("shows daemon errors instead of claiming reload success", async () => {
    invoke.mockRejectedValue(new Error("SCRIPT_INTERRUPTED"));
    await act(async () => { render(<ScriptingSection />); });
    expect(screen.getByRole("alert").textContent).toContain("SCRIPT_INTERRUPTED");
  });

  it("forwards only registered chords in their explicit precedence phase", async () => {
    const state = { enabled: true, log: "", bindings: [{ keys: "cmd+shift+a", override: false }] };
    invoke.mockResolvedValue(state);
    function Focus() { useScripting("backend-session"); return null; }
    await act(async () => { render(<Focus />); });
    const event = () => new KeyboardEvent("keydown", { key: "A", metaKey: true, shiftKey: true, cancelable: true });
    expect(dispatchScriptKey(event(), true)).toBe(false);
    const unhandled = event();
    expect(dispatchScriptKey(unhandled, false)).toBe(true);
    expect(unhandled.defaultPrevented).toBe(true);
    expect(invoke).toHaveBeenLastCalledWith("cmd_script", { action: { action: "key", keys: "cmd+shift+a", session: "backend-session" } });
    const handled = event(); handled.preventDefault();
    expect(dispatchScriptKey(handled, false)).toBe(false);
    invoke.mockResolvedValue({ ...state, bindings: [{ keys: "cmd+shift+a", override: true }] });
    await scriptControl({ action: "status" });
    expect(dispatchScriptKey(event(), true)).toBe(true);
    expect(dispatchScriptKey(new KeyboardEvent("keydown", { key: "b" }), false)).toBe(false);
  });
});
