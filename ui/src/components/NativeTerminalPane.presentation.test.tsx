import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NativeTerminalPane, resetNativeTerminalPaneForTest } from "./NativeTerminalPane";
import { resetNativeTerminalLifecycleForTest } from "../lib/nativeTerminalLifecycle";
import type { TerminalSession } from "../lib/types";
import * as shortcuts from "../lib/shortcuts";

const bridge = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: bridge.invoke,
  isTauri: () => true,
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onDragDropEvent: async () => () => undefined }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async () => () => undefined,
}));
vi.mock("../lib/tauri", async (original) => ({
  ...await original<typeof import("../lib/tauri")>(),
  onNativeTerminalFocus: async () => () => undefined,
  onNativeTerminalPaste: async () => () => undefined,
  onNativeTerminalCopyOrInterrupt: async () => () => undefined,
  onNativeTerminalScrollbar: async () => () => undefined,
  setNativeTerminalScrollbarOverlay: async () => undefined,
  setNativeTerminalAttentionFrame: async () => undefined,
}));

const PRESENTED = {
  presented: true,
  cursorCol: 0,
  cursorRow: 0,
  cellWidthPx: 8,
  cellHeightPx: 16,
};

function session(backendSessionId: string | null = "backend-a"): TerminalSession {
  return {
    id: "pane-a",
    cwd: "/workspace",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId,
    lifecycle: backendSessionId ? "working" : "exited",
  };
}

function commands(name: string) {
  return bridge.invoke.mock.calls.filter(([command]) => command === name);
}

beforeEach(() => {
  vi.stubGlobal("navigator", { platform: "MacIntel", userAgent: "Macintosh" });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect")
    .mockReturnValue(new DOMRect(10, 20, 800, 600));
  resetNativeTerminalLifecycleForTest();
  resetNativeTerminalPaneForTest();
  bridge.invoke.mockReset();
  bridge.invoke.mockImplementation(async (command) =>
    command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined,
  );
});

afterEach(async () => {
  await act(async () => { cleanup(); });
  document.querySelectorAll('[data-presentation-test-overlay]').forEach((node) => node.remove());
  resetNativeTerminalLifecycleForTest();
  resetNativeTerminalPaneForTest();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function dialog() {
  const overlay = document.createElement("div");
  overlay.setAttribute("role", "dialog");
  overlay.setAttribute("data-presentation-test-overlay", "");
  return overlay;
}

describe("native terminal presentation retention", () => {
  it("does not steal dialog focus when an earlier attachment finishes", async () => {
    let finishAttach: () => void = () => undefined;
    const attachment = new Promise<void>((resolve) => { finishAttach = resolve; });
    bridge.invoke.mockImplementation(async (command) => {
      if (command === "cmd_native_terminal_attach") return attachment;
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    render(<NativeTerminalPane session={session()} />);
    await act(async () => {});
    const overlay = dialog();
    const close = document.createElement("button");
    overlay.appendChild(close);
    await act(async () => {
      document.body.appendChild(overlay);
      close.focus();
    });
    await act(async () => { finishAttach(); });
    expect(document.activeElement).toBe(close);
    expect(commands("cmd_native_terminal_detach")).toHaveLength(0);
  });

  it("keeps a presented macOS surface under a dialog without accepting terminal keys", async () => {
    const view = render(<NativeTerminalPane session={session()} active />);
    await act(async () => {});
    expect(commands("cmd_native_terminal_set_bounds")).toHaveLength(1);
    await act(async () => { document.body.appendChild(dialog()); });
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute("data-native-terminal-visible", "true");
    expect(commands("cmd_native_terminal_detach")).toHaveLength(0);

    await act(async () => {
      fireEvent.keyDown(view.getByTestId("native-terminal-focus-sink"), { key: "x", code: "KeyX" });
    });
    expect(commands("cmd_native_terminal_send_input")).toHaveLength(0);
  });

  it("does not recover an in-flight input after a dialog takes ownership", async () => {
    let rejectInput: (error: unknown) => void = () => undefined;
    const inputResult = new Promise<never>((_resolve, reject) => { rejectInput = reject; });
    bridge.invoke.mockImplementation(async (command) => {
      if (command === "cmd_native_terminal_send_input") return inputResult;
      return command === "cmd_native_terminal_set_bounds" ? PRESENTED : undefined;
    });
    const view = render(<NativeTerminalPane session={session()} active />);
    await act(async () => {});
    fireEvent.keyDown(view.getByTestId("native-terminal-focus-sink"), { key: "x", code: "KeyX" });
    expect(commands("cmd_native_terminal_send_input")).toHaveLength(1);
    await act(async () => { document.body.appendChild(dialog()); });
    await act(async () => {
      rejectInput({ code: "SESSION_NOT_FOUND", message: "Gone", details: { inputWritten: false } });
    });
    expect(commands("cmd_native_terminal_attach")).toHaveLength(1);
    expect(commands("cmd_native_terminal_send_input")).toHaveLength(1);
  });

  it("retains a shown final frame on exit, blocks input, and releases it on unmount", async () => {
    const view = render(<NativeTerminalPane session={session()} active />);
    await act(async () => {});
    expect(commands("cmd_native_terminal_set_bounds")).toHaveLength(1);
    await act(async () => { view.rerender(<NativeTerminalPane session={session(null)} active />); });

    expect(commands("cmd_native_terminal_detach")).toHaveLength(0);
    expect(commands("cmd_native_terminal_attach")).toHaveLength(1);
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute("data-native-terminal-presented", "true");
    await act(async () => {
      fireEvent.keyDown(view.getByTestId("native-terminal-focus-sink"), { key: "x", code: "KeyX" });
    });
    expect(commands("cmd_native_terminal_send_input")).toHaveLength(0);
    await act(async () => { view.unmount(); });
    expect(commands("cmd_native_terminal_detach")).toEqual([
      ["cmd_native_terminal_detach", { sessionId: "backend-a" }],
    ]);
  });

  it("does not attach a cold exited pane and leaves an opaque fallback", async () => {
    const view = render(<NativeTerminalPane session={session(null)} />);
    await act(async () => {});
    expect(commands("cmd_native_terminal_attach")).toHaveLength(0);
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute("data-native-terminal-presented", "false");
  });

  it("yields an exited native surface outside macOS so the disconnect overlay is not occluded", async () => {
    vi.spyOn(shortcuts, "isMacShortcutPlatform").mockReturnValue(false);
    const view = render(<NativeTerminalPane session={session()} />);
    await act(async () => {});
    await act(async () => { view.rerender(<NativeTerminalPane session={session(null)} />); });
    expect(commands("cmd_native_terminal_detach")).toHaveLength(1);
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute("data-native-terminal-presented", "false");
  });

  it("never reattaches a dead PTY when retained-frame geometry recovery is requested", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    const view = render(<NativeTerminalPane session={session()} />);
    await act(async () => {});
    await act(async () => { view.rerender(<NativeTerminalPane session={session(null)} />); });
    bridge.invoke.mockImplementation(async (command) => {
      if (command === "cmd_native_terminal_set_bounds") {
        throw { code: "INTERNAL_ERROR", message: "Surface resize failed" };
      }
      return undefined;
    });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockReturnValue(new DOMRect(10, 20, 900, 600));
    await act(async () => { window.dispatchEvent(new Event("resize")); });
    await act(async () => { fireEvent.click(view.getByRole("alert")); });
    expect(commands("cmd_native_terminal_attach")).toHaveLength(1);
  });

  it("does not retain a first frame that arrives after the session exited", async () => {
    let finishBounds: (value: typeof PRESENTED) => void = () => undefined;
    const bounds = new Promise<typeof PRESENTED>((resolve) => { finishBounds = resolve; });
    bridge.invoke.mockImplementation(async (command) =>
      command === "cmd_native_terminal_set_bounds" ? bounds : undefined,
    );
    const view = render(<NativeTerminalPane session={session()} />);
    await act(async () => {});
    await act(async () => { view.rerender(<NativeTerminalPane session={session(null)} />); });
    await act(async () => { finishBounds(PRESENTED); });
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute("data-native-terminal-presented", "false");
    expect(commands("cmd_native_terminal_attach")).toHaveLength(1);
    expect(commands("cmd_native_terminal_detach")).toHaveLength(1);
  });

  it("holds the final frame until a reconnected replacement is presented", async () => {
    let presentReplacement: (value: typeof PRESENTED) => void = () => undefined;
    const replacement = new Promise<typeof PRESENTED>((resolve) => { presentReplacement = resolve; });
    bridge.invoke.mockImplementation(async (command, args) => {
      if (command !== "cmd_native_terminal_set_bounds") return undefined;
      return args?.sessionId === "backend-b" ? replacement : PRESENTED;
    });
    const view = render(<NativeTerminalPane session={session()} />);
    await act(async () => {});
    await act(async () => { view.rerender(<NativeTerminalPane session={session(null)} />); });
    await act(async () => { view.rerender(<NativeTerminalPane session={session("backend-b")} />); });
    expect(commands("cmd_native_terminal_detach")).toHaveLength(0);

    await act(async () => { presentReplacement(PRESENTED); });
    expect(commands("cmd_native_terminal_detach")).toEqual([
      ["cmd_native_terminal_detach", { sessionId: "backend-a" }],
    ]);
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute("data-native-terminal-presented", "true");
  });
});
