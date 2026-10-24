import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TerminalSession } from "../lib/types";
import { FALLBACK_PREFERENCES } from "../lib/terminalSettings";
import { resetNativeTerminalLifecycleForTest, registerDurableNativeBinding } from "../lib/nativeTerminalLifecycle";
import { resetNativeTerminalPaneForTest } from "./NativeTerminalPane";
import { TerminalPane } from "./TerminalPane";

const tauriCoreMocks = vi.hoisted(() => ({
  invoke: vi.fn<(cmd: string, args?: any) => Promise<any>>(async () => undefined),
  isTauri: vi.fn(() => true),
  channels: [] as Array<{ onmessage: (frame: string) => void }>,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCoreMocks.invoke,
  isTauri: tauriCoreMocks.isTauri,
  Channel: class {
    onmessage: (frame: string) => void;
    constructor(onmessage: (frame: string) => void) {
      this.onmessage = onmessage;
      tauriCoreMocks.channels.push(this);
    }
  },
}));
vi.mock("../lib/localSplitLifecycle", async (original) => ({
  ...await original<typeof import("../lib/localSplitLifecycle")>(),
  persistNativeBinding: async () => undefined,
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onDragDropEvent: async () => () => undefined }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async () => () => undefined,
}));
vi.mock("../lib/tauri", async (original) => ({
  ...(await original<typeof import("../lib/tauri")>()),
  onNativeTerminalFocus: async () => () => undefined,
  onNativeTerminalPaste: async () => () => undefined,
  onNativeTerminalCopyOrInterrupt: async () => () => undefined,
  onNativeTerminalScrollbar: async () => () => undefined,
  setNativeTerminalScrollbarOverlay: async () => undefined,
  setNativeTerminalAttentionFrame: async () => undefined,
}));

const PRESENTED = { presented: true, cursorCol: 0, cursorRow: 0, cellWidthPx: 8, cellHeightPx: 16 };
const BANNER = "GPU unavailable - using compatibility renderer";

const attachTuple = {
  backendSessionId: "backend-no-gpu",
  daemonEpoch: "epoch-1",
  incarnation: "inc-1",
  frontendSessionId: "pane-no-gpu",
  paneIdentity: "pane-no-gpu",
  bindingKey: "backend-no-gpu:epoch-1:0::inc-1",
  attemptGeneration: 0,
};

const session: TerminalSession = {
  id: "pane-no-gpu",
  cwd: "/repo/path",
  workspaceId: "ws-local",
  worktree: { wsId: "ws-local", slug: "main" },
  backendSessionId: "backend-no-gpu",
  lifecycle: "working",
  daemonEpoch: "epoch-1",
  incarnation: "inc-1",
  attachTuple,
};

function gridFrame(text: string): string {
  return JSON.stringify({
    type: "grid",
    cols: 20,
    rows: 2,
    cursor: { x: text.length, y: 0, visible: true, blinking: false, wideTail: false, visualStyle: "block" },
    lines: [{ index: 0, runs: [{ text, fg: null, bg: null, attrs: 0, cells: text.length }] }, { index: 1, runs: [] }],
  });
}

function mockCommands(attach: () => Promise<unknown>) {
  tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
    if (cmd === "cmd_native_terminal_attach") return attach();
    if (cmd === "cmd_native_terminal_set_bounds") return PRESENTED;
    if (cmd === "cmd_terminal_preferences") return FALLBACK_PREFERENCES;
    if (cmd === "cmd_terminal_grid_attach") return "grid-stream-1";
    return undefined;
  });
}

describe("TerminalPane GPU fallback", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("navigator", { platform: "MacIntel", userAgent: "Macintosh" });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(new DOMRect(10, 20, 800, 600));
    resetNativeTerminalLifecycleForTest();
    resetNativeTerminalPaneForTest();
    tauriCoreMocks.channels.length = 0;
    tauriCoreMocks.invoke.mockReset();
    tauriCoreMocks.isTauri.mockReturnValue(true);
    registerDurableNativeBinding(attachTuple);
  });

  afterEach(async () => {
    await act(async () => {
      cleanup();
    });
    vi.clearAllTimers();
    vi.useRealTimers();
    resetNativeTerminalLifecycleForTest();
    resetNativeTerminalPaneForTest();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("mounts the DOM grid renderer and the banner when the native renderer is unavailable", async () => {
    mockCommands(async () => {
      throw {
        code: "NATIVE_RENDERER_UNAVAILABLE",
        message: "GPU adapter request failed: none",
        details: { source: "wgpu", kind: "renderer_unavailable" },
      };
    });

    render(<TerminalPane session={session} active={true} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    expect(screen.getByTestId("terminal-gpu-fallback-banner")).toHaveTextContent(BANNER);
    expect(screen.getByTestId("remote-terminal-grid")).toBeInTheDocument();
    expect(screen.queryByTestId("native-terminal-pane")).not.toBeInTheDocument();
    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith(
      "cmd_terminal_grid_attach",
      expect.objectContaining({ sessionId: "backend-no-gpu" }),
    );
    // The session is never closed or detached by the fallback.
    const commands = tauriCoreMocks.invoke.mock.calls.map(([cmd]) => cmd);
    expect(commands).not.toContain("cmd_terminal_close");
    expect(commands).not.toContain("cmd_native_terminal_close");

    // Frames from the local session's grid stream render in the DOM grid.
    await act(async () => {
      tauriCoreMocks.channels.at(-1)?.onmessage(gridFrame("FALLBACK_OK"));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.getByTestId("remote-terminal-grid")).toHaveTextContent("FALLBACK_OK");
  });

  it("keeps the native surface and shows no banner when a GPU adapter exists", async () => {
    mockCommands(async () => ({ sessionId: "backend-no-gpu", attachTuple, presented: false }));

    render(<TerminalPane session={session} active={true} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    expect(screen.getByTestId("native-terminal-pane")).toBeInTheDocument();
    expect(screen.queryByTestId("terminal-gpu-fallback-banner")).not.toBeInTheDocument();
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith("cmd_terminal_grid_attach", expect.anything());
  });

  it("falls back when the attached surface reports it cannot present", async () => {
    // Driverless host: a software adapter exists, attach succeeds, then the window surface
    // has no 8-bit RGBA/BGRA format and bounds updates report the renderer unavailable.
    tauriCoreMocks.invoke.mockImplementation(async (cmd) => {
      if (cmd === "cmd_native_terminal_attach") return { sessionId: "backend-no-gpu", attachTuple, presented: false };
      if (cmd === "cmd_native_terminal_set_bounds") {
        throw {
          code: "NATIVE_RENDERER_UNAVAILABLE",
          message: "GPU surface presentation unsupported: Surface does not support standard 8-bit RGBA/BGRA formats",
          details: { source: "wgpu", kind: "renderer_unavailable" },
        };
      }
      if (cmd === "cmd_terminal_preferences") return FALLBACK_PREFERENCES;
      if (cmd === "cmd_terminal_grid_attach") return "grid-stream-1";
      return undefined;
    });

    render(<TerminalPane session={session} active={true} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    expect(tauriCoreMocks.invoke).toHaveBeenCalledWith("cmd_native_terminal_set_bounds", expect.anything());
    expect(screen.getByTestId("terminal-gpu-fallback-banner")).toHaveTextContent(BANNER);
    expect(screen.getByTestId("remote-terminal-grid")).toBeInTheDocument();
    expect(screen.queryByTestId("native-terminal-pane")).not.toBeInTheDocument();
    const commands = tauriCoreMocks.invoke.mock.calls.map(([cmd]) => cmd);
    expect(commands).not.toContain("cmd_terminal_close");
  });
});
