import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TerminalSession } from "../lib/types";
import { TerminalPane } from "./TerminalPane";

const manager = vi.hoisted(() => ({
  applySettings: vi.fn(),
  applyInstanceSettings: vi.fn(),
  updateSession: vi.fn(),
  getOrCreate: vi.fn(),
  getInstance: vi.fn(),
  registerVisible: vi.fn(() => vi.fn()),
}));

vi.mock("../lib/terminalHostManager", () => ({ terminalHostManager: manager }));
vi.mock("../lib/terminalSettings", () => ({
  useTerminalSettings: () => ({ settings: { fontSize: 13, theme: { background: "#282c34" } } }),
}));

type ResizeRecord = {
  callback: ResizeObserverCallback;
  observe: ReturnType<typeof vi.fn>;
  disconnect: ReturnType<typeof vi.fn>;
};

const resizeRecords: ResizeRecord[] = [];
let fontsDescriptor: PropertyDescriptor | undefined;

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
    manager.applyInstanceSettings.mockReset();
    manager.updateSession.mockReset();
    manager.getOrCreate.mockReset();
    manager.getInstance.mockReset();
    manager.registerVisible.mockReset().mockImplementation(() => vi.fn());
    resizeRecords.length = 0;
    fontsDescriptor = Object.getOwnPropertyDescriptor(document, "fonts");
    vi.stubGlobal("ResizeObserver", TestResizeObserver);
  });

  afterEach(() => {
    if (fontsDescriptor) {
      Object.defineProperty(document, "fonts", fontsDescriptor);
    } else {
      Reflect.deleteProperty(document, "fonts");
    }
    cleanup();
    vi.unstubAllGlobals();
  });

  it("mounts the terminal element and focuses when active without duplicate ResizeObserver in pane", async () => {
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

    await waitFor(() => expect(manager.getOrCreate).toHaveBeenCalledOnce());
    expect(element.parentElement).toBe(mount);
    // Sizing/ResizeObserver is owned by terminalHostManager; pane must not create a duplicate observer
    expect(resizeRecords).toHaveLength(0);
    expect(focus).toHaveBeenCalled();
  });

  it("applies settings only to its own instance on mount and does not invoke global applySettings across all instances", async () => {
    const element = document.createElement("div");
    element.className = "terminal-host";
    const instance = {
      element,
      fitAddon: { fit: vi.fn() },
      terminal: { focus: vi.fn() },
      active: true,
      session: session(),
    };
    manager.getOrCreate.mockResolvedValue(instance);
    manager.getInstance.mockReturnValue(instance);

    render(<TerminalPane session={session()} active />);

    await waitFor(() => expect(manager.getOrCreate).toHaveBeenCalledOnce());
    expect(manager.applyInstanceSettings).toHaveBeenCalledWith("session-new", expect.objectContaining({ fontSize: 13 }));
    expect(manager.applySettings).not.toHaveBeenCalled();
  });

  it("fills unused terminal rows with the active terminal theme background", () => {
    manager.getOrCreate.mockResolvedValue({
      element: document.createElement("div"),
      fitAddon: { fit: vi.fn() },
      terminal: { focus: vi.fn() },
      active: true,
      session: session(),
    });

    render(<TerminalPane session={session()} active />);

    expect(screen.getByTestId("terminal-pane-surface")).toHaveStyle({ backgroundColor: "#282c34" });
    expect(screen.getByTestId("terminal-mount")).toHaveStyle({ backgroundColor: "#282c34" });
  });

  it("realigns a bottom-following terminal after the post-mount font fit shifts it by one row", async () => {
    Object.defineProperty(document, "fonts", {
      configurable: true,
      value: { ready: Promise.resolve() },
    });

    const element = document.createElement("div");
    Object.defineProperties(element, {
      clientHeight: { configurable: true, value: 480 },
      clientWidth: { configurable: true, value: 640 },
    });
    const buffer = { active: { baseY: 12, viewportY: 12 } };
    const terminal = {
      buffer,
      focus: vi.fn(),
      scrollToBottom: vi.fn(() => {
        buffer.active.viewportY = buffer.active.baseY;
      }),
    };
    const fit = vi.fn(() => {
      buffer.active.viewportY = buffer.active.baseY - 1;
    });
    const instance = {
      element,
      fitAddon: { fit },
      terminal,
      active: true,
      session: session(),
    };
    manager.getOrCreate.mockResolvedValue(instance);
    manager.getInstance.mockReturnValue(instance);

    render(<TerminalPane session={session()} active />);

    await waitFor(() => expect(fit).toHaveBeenCalledOnce());
    expect(terminal.scrollToBottom).toHaveBeenCalledOnce();
    expect(buffer.active.viewportY).toBe(buffer.active.baseY);
  });

  it("preserves user scrollback after the post-mount font fit", async () => {
    Object.defineProperty(document, "fonts", {
      configurable: true,
      value: { ready: Promise.resolve() },
    });

    const element = document.createElement("div");
    Object.defineProperties(element, {
      clientHeight: { configurable: true, value: 480 },
      clientWidth: { configurable: true, value: 640 },
    });
    const buffer = { active: { baseY: 12, viewportY: 4 } };
    const terminal = {
      buffer,
      focus: vi.fn(),
      scrollToBottom: vi.fn(),
    };
    const fit = vi.fn();
    const instance = {
      element,
      fitAddon: { fit },
      terminal,
      active: true,
      session: session(),
    };
    manager.getOrCreate.mockResolvedValue(instance);
    manager.getInstance.mockReturnValue(instance);

    render(<TerminalPane session={session()} active />);

    await waitFor(() => expect(fit).toHaveBeenCalledOnce());
    expect(terminal.scrollToBottom).not.toHaveBeenCalled();
    expect(buffer.active.viewportY).toBe(4);
  });
});
