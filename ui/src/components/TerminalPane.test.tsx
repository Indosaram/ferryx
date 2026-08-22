import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TerminalSession } from "../lib/types";
import { TerminalPane } from "./TerminalPane";

const manager = vi.hoisted(() => ({
  applySettings: vi.fn(),
  updateSession: vi.fn(),
  getOrCreate: vi.fn(),
  getInstance: vi.fn(),
}));

vi.mock("../lib/terminalHostManager", () => ({ terminalHostManager: manager }));
vi.mock("../lib/terminalSettings", () => ({
  useTerminalSettings: () => ({ settings: { fontSize: 13 } }),
}));

type ResizeRecord = {
  callback: ResizeObserverCallback;
  observe: ReturnType<typeof vi.fn>;
  disconnect: ReturnType<typeof vi.fn>;
};

const resizeRecords: ResizeRecord[] = [];

class TestResizeObserver {
  readonly observe = vi.fn();
  readonly unobserve = vi.fn();
  readonly disconnect = vi.fn();

  constructor(readonly callback: ResizeObserverCallback) {
    resizeRecords.push({ callback, observe: this.observe, disconnect: this.disconnect });
  }
}

function session(): TerminalSession {
  return {
    id: "session-new",
    cwd: "/repo/main",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: "backend-new",
    lifecycle: "working",
  };
}

describe("TerminalPane mounted sizing", () => {
  beforeEach(() => {
    manager.applySettings.mockReset();
    manager.updateSession.mockReset();
    manager.getOrCreate.mockReset();
    manager.getInstance.mockReset();
    resizeRecords.length = 0;
    vi.stubGlobal("ResizeObserver", TestResizeObserver);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("waits for a measurable mounted container and then fits the new terminal to the full pane", async () => {
    const element = document.createElement("div");
    element.className = "terminal-host";
    const fit = vi.fn();
    const focus = vi.fn();
    const instance = {
      element,
      fitAddon: { fit },
      terminal: { focus },
      active: true,
      session: session(),
    };
    manager.getOrCreate.mockResolvedValue(instance);
    manager.getInstance.mockReturnValue(instance);

    render(<TerminalPane session={session()} active />);
    const mount = screen.getByTestId("terminal-mount");
    let width = 0;
    let height = 0;
    vi.spyOn(mount, "getBoundingClientRect").mockImplementation(
      () =>
        ({
          left: 0,
          top: 0,
          right: width,
          bottom: height,
          width,
          height,
          x: 0,
          y: 0,
          toJSON: () => ({}),
        }) as DOMRect,
    );

    await waitFor(() => expect(manager.getOrCreate).toHaveBeenCalledOnce());
    await waitFor(() => expect(resizeRecords).toHaveLength(1));
    expect(resizeRecords[0].observe).toHaveBeenCalledWith(mount);
    expect(element.parentElement).toBe(mount);
    expect(fit).not.toHaveBeenCalled();

    width = 960;
    height = 640;
    act(() => {
      resizeRecords[0].callback([], {} as ResizeObserver);
    });

    await waitFor(() => expect(fit).toHaveBeenCalled());
    expect(element.style.position).toBe("absolute");
    expect(element.style.inset).toBe("0px");
    expect(element.style.width).toBe("100%");
    expect(element.style.height).toBe("100%");
    expect(element.style.minWidth).toBe("0px");
    expect(element.style.minHeight).toBe("0px");
  });
});
