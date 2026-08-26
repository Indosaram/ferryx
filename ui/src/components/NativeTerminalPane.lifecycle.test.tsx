import { StrictMode } from "react";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { NativeTerminalVisibilityProvider } from "../lib/nativeTerminalVisibility";
import type { TerminalSession } from "../lib/types";
import { NativeTerminalPane } from "./NativeTerminalPane";

const tauriCoreMocks = vi.hoisted(() => ({
  invoke: vi.fn<(cmd: string, args?: any) => Promise<any>>(async () => undefined),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCoreMocks.invoke,
  isTauri: tauriCoreMocks.isTauri,
}));

class TestResizeObserver implements ResizeObserver {
  readonly observe = vi.fn();
  readonly unobserve = vi.fn();
  readonly disconnect = vi.fn();

  constructor(readonly callback: ResizeObserverCallback) {}
}

function session(id: string): TerminalSession {
  return {
    id: `frontend-${id}`,
    cwd: "/workspace/orca-lite",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: id,
    lifecycle: "working",
  };
}

function lifecycleCalls(): Array<[string, string | undefined]> {
  return tauriCoreMocks.invoke.mock.calls
    .filter(([cmd]) => cmd === "cmd_native_terminal_attach" || cmd === "cmd_native_terminal_detach")
    .map(([cmd, args]) => [cmd, args?.sessionId]);
}

// jsdom reports an all-zero rect, which the compositor rejects as invalid
// dimensions, so panes under test must measure a real area to reach set_bounds.
const PANE_RECT = {
  x: 10,
  y: 20,
  width: 800,
  height: 600,
  top: 20,
  bottom: 620,
  left: 10,
  right: 810,
  toJSON: () => ({}),
} as DOMRect;

describe("NativeTerminalPane compositor ownership lifecycle", () => {
  let restorePaneRect: () => void;

  beforeEach(() => {
    const originalRect = HTMLElement.prototype.getBoundingClientRect;
    HTMLElement.prototype.getBoundingClientRect = function () {
      return PANE_RECT;
    };
    restorePaneRect = () => {
      HTMLElement.prototype.getBoundingClientRect = originalRect;
    };
    tauriCoreMocks.invoke.mockReset();
    tauriCoreMocks.invoke.mockResolvedValue(undefined);
    tauriCoreMocks.isTauri.mockReset();
    tauriCoreMocks.isTauri.mockReturnValue(true);
    vi.stubGlobal("ResizeObserver", TestResizeObserver);
    document.querySelectorAll('[role="dialog"]').forEach((node) => node.remove());
  });

  afterEach(async () => {
    restorePaneRect();
    cleanup();
    await Promise.resolve();
    vi.unstubAllGlobals();
  });

  it("does not block a new split pane attach behind an unrelated session", async () => {
    let resolveOldAttach: (() => void) | undefined;
    const oldAttach = new Promise<void>((resolve) => {
      resolveOldAttach = resolve;
    });

    tauriCoreMocks.invoke.mockImplementation(async (cmd, args) => {
      if (cmd === "cmd_native_terminal_attach" && args?.sessionId === "backend-a") {
        await oldAttach;
      }
      return undefined;
    });

    const view = render(<NativeTerminalPane session={session("backend-a")} />);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-a"]]);
    });

    view.rerender(<NativeTerminalPane session={session("backend-b")} />);
    await Promise.resolve();

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-a"],
        ["cmd_native_terminal_attach", "backend-b"],
      ]);
    });

    resolveOldAttach?.();

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-a"],
        ["cmd_native_terminal_attach", "backend-b"],
        ["cmd_native_terminal_detach", "backend-a"],
      ]);
    });
  });

  it("keeps the existing surface attached when a split reparents its pane", async () => {
    const view = render(<NativeTerminalPane session={session("backend-reparented")} />);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-reparented"]]);
    });

    view.rerender(
      <div>
        <NativeTerminalPane session={session("backend-reparented")} />
      </div>,
    );

    await Promise.resolve();

    expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-reparented"]]);
  });

  it("detaches and blocks input while the full-screen Settings surface covers the active pane", async () => {
    const view = render(<NativeTerminalPane session={session("backend-settings")} />);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-settings"]]);
    });
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
      "data-native-terminal-visible",
      "true",
    );

    const settings = document.createElement("div");
    settings.setAttribute("role", "dialog");
    settings.setAttribute("aria-label", "Settings");
    document.body.appendChild(settings);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-settings"],
        ["cmd_native_terminal_detach", "backend-settings"],
      ]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "false",
      );
    });

    tauriCoreMocks.invoke.mockClear();
    const sink = view.getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;
    fireEvent.input(sink, { target: { value: "must-not-reach-pty" } });
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.anything(),
    );

    settings.remove();
    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-settings"]]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "true",
      );
    });
  });

  it("detaches and blocks input while a mounted role=dialog New Tab menu covers the active pane", async () => {
    const view = render(<NativeTerminalPane session={session("backend-newtab")} />);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-newtab"]]);
    });
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
      "data-native-terminal-visible",
      "true",
    );

    const newTabMenu = document.createElement("div");
    newTabMenu.setAttribute("role", "dialog");
    newTabMenu.setAttribute("aria-label", "New tab menu");
    document.body.appendChild(newTabMenu);

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-newtab"],
        ["cmd_native_terminal_detach", "backend-newtab"],
      ]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "false",
      );
    });

    tauriCoreMocks.invoke.mockClear();
    const sink = view.getByTestId("native-terminal-focus-sink") as HTMLTextAreaElement;
    fireEvent.input(sink, { target: { value: "must-not-reach-pty" } });
    expect(tauriCoreMocks.invoke).not.toHaveBeenCalledWith(
      "cmd_native_terminal_send_input",
      expect.anything(),
    );

    newTabMenu.remove();
    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-newtab"]]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "true",
      );
    });
  });

  it("suppresses and restores native surface via NativeTerminalVisibilityProvider", async () => {
    const view = render(
      <NativeTerminalVisibilityProvider visible={false}>
        <NativeTerminalPane session={session("backend-provider")} />
      </NativeTerminalVisibilityProvider>,
    );

    await Promise.resolve();
    expect(lifecycleCalls()).toEqual([]);
    expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
      "data-native-terminal-visible",
      "false",
    );

    view.rerender(
      <NativeTerminalVisibilityProvider visible={true}>
        <NativeTerminalPane session={session("backend-provider")} />
      </NativeTerminalVisibilityProvider>,
    );

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([["cmd_native_terminal_attach", "backend-provider"]]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "true",
      );
    });

    view.rerender(
      <NativeTerminalVisibilityProvider visible={false}>
        <NativeTerminalPane session={session("backend-provider")} />
      </NativeTerminalVisibilityProvider>,
    );

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-provider"],
        ["cmd_native_terminal_detach", "backend-provider"],
      ]);
      expect(view.getByTestId("native-terminal-pane")).toHaveAttribute(
        "data-native-terminal-visible",
        "false",
      );
    });
  });

  it("handles React StrictMode remount cleanly preserving attach sequence", async () => {
    render(
      <StrictMode>
        <NativeTerminalPane session={session("backend-strict")} />
      </StrictMode>,
    );

    await waitFor(() => {
      expect(lifecycleCalls()).toEqual([
        ["cmd_native_terminal_attach", "backend-strict"],
      ]);
    });
  });
});
