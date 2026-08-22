import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  let bellHandler: (() => void) | null = null;
  let titleHandler: ((title: string) => void) | null = null;
  let dataHandler: ((data: string) => void) | null = null;
  const terminalWrites: string[] = [];
  const terminalDisposed = vi.fn();
  const searchAddonDisposed = vi.fn();

  class MockTerminal {
    cols = 80;
    rows = 24;
    unicode = { activeVersion: "6" };
    loadAddon = vi.fn();
    open = vi.fn();
    focus = vi.fn();
    dispose = terminalDisposed;
    write = vi.fn((text: string) => {
      terminalWrites.push(text);
    });
    writeln = vi.fn((text: string) => {
      terminalWrites.push(`${text}\n`);
    });
    onBell(handler: () => void) {
      bellHandler = handler;
      return { dispose: vi.fn() };
    }
    onTitleChange(handler: (title: string) => void) {
      titleHandler = handler;
      return { dispose: vi.fn() };
    }
    onData(handler: (data: string) => void) {
      dataHandler = handler;
      return { dispose: vi.fn() };
    }
  }

  class MockFitAddon {
    fit = vi.fn();
  }

  class MockSearchAddon {
    dispose = searchAddonDisposed;
    findNext = vi.fn();
    findPrevious = vi.fn();
    clearDecorations = vi.fn();
  }

  const outputListeners = new Map<string, Set<(text: string) => void>>();

  return {
    MockTerminal,
    MockFitAddon,
    MockSearchAddon,
    terminalWrites,
    terminalDisposed,
    searchAddonDisposed,
    outputListeners,
    triggerBell: () => bellHandler?.(),
    triggerTitleChange: (title: string) => titleHandler?.(title),
    triggerData: (data: string) => dataHandler?.(data),
    emitSessionOutput: (sessionId: string, text: string) => {
      for (const cb of outputListeners.get(sessionId) ?? []) {
        cb(text);
      }
    },
    resetTerminalMocks: () => {
      bellHandler = null;
      titleHandler = null;
      dataHandler = null;
      terminalWrites.length = 0;
      terminalDisposed.mockClear();
      searchAddonDisposed.mockClear();
      outputListeners.clear();
    },
  };
});

vi.mock("@xterm/xterm", () => ({
  Terminal: mocks.MockTerminal,
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: mocks.MockFitAddon,
}));

vi.mock("@xterm/addon-search", () => ({
  SearchAddon: mocks.MockSearchAddon,
}));

vi.mock("@xterm/addon-unicode11", () => ({
  Unicode11Addon: class MockUnicode11Addon {},
}));

vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class MockWebglAddon {
    onContextLoss = vi.fn();
    dispose = vi.fn();
  },
}));

vi.mock("./tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./tauri")>();
  return {
    ...actual,
    isTauriRuntime: vi.fn(() => false),
    resizeTerminal: vi.fn().mockResolvedValue(undefined),
    writeTerminal: vi.fn().mockResolvedValue(undefined),
    fetchCachedNativePreferences: vi.fn().mockResolvedValue(null),
  };
});

vi.mock("./terminalEvents", () => ({
  terminalEventBus: {
    subscribeOutput: vi.fn((sessionId: string, cb: (text: string) => void) => {
      let set = mocks.outputListeners.get(sessionId);
      if (!set) {
        set = new Set();
        mocks.outputListeners.set(sessionId, set);
      }
      set.add(cb);
      return vi.fn(() => {
        set?.delete(cb);
        if (set?.size === 0) mocks.outputListeners.delete(sessionId);
      });
    }),
  },
}));

class MockResizeObserver {
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
}

vi.stubGlobal("ResizeObserver", MockResizeObserver);

import { terminalHostManager } from "./terminalHostManager";
import type { TerminalSession } from "./types";

function createSession(id = "sess-1", backendSessionId = "backend-1"): TerminalSession {
  return {
    id,
    cwd: "/workspace/orca",
    workspaceId: "ws-1",
    worktree: { wsId: "ws-1", slug: "main" },
    backendSessionId,
    lifecycle: "working",
  };
}

describe("terminalHostManager", () => {
  beforeEach(() => {
    mocks.resetTerminalMocks();
  });

  it("renders Ferryx branding in non-Tauri preview banner", async () => {
    const session = createSession("sess-banner");
    await terminalHostManager.getOrCreate(session, true);

    const bannerText = mocks.terminalWrites.join("");
    expect(bannerText).toContain("Ferryx");
    expect(bannerText).not.toContain("rorca");
    terminalHostManager.destroy("sess-banner");
  });

  it("fires onBell and onTitleChange exactly once and does not fire stale initial closures when cleared", async () => {
    const session = createSession("sess-events");
    const onBell1 = vi.fn();
    const onTitle1 = vi.fn();

    await terminalHostManager.getOrCreate(session, true, onBell1, onTitle1);

    mocks.triggerBell();
    expect(onBell1).toHaveBeenCalledTimes(1);

    mocks.triggerTitleChange("new title 1");
    expect(onTitle1).toHaveBeenCalledTimes(1);
    expect(onTitle1).toHaveBeenCalledWith("new title 1");

    // Re-register with new callbacks (e.g. component re-render)
    const onBell2 = vi.fn();
    const onTitle2 = vi.fn();
    await terminalHostManager.getOrCreate(session, true, onBell2, onTitle2);

    mocks.triggerBell();
    expect(onBell1).toHaveBeenCalledTimes(1); // Should not fire stale callback
    expect(onBell2).toHaveBeenCalledTimes(1);

    mocks.triggerTitleChange("new title 2");
    expect(onTitle1).toHaveBeenCalledTimes(1); // Should not fire stale callback
    expect(onTitle2).toHaveBeenCalledTimes(1);
    expect(onTitle2).toHaveBeenCalledWith("new title 2");

    // When cleared, neither stale callback should fire
    await terminalHostManager.getOrCreate(session, true, undefined, undefined);
    mocks.triggerBell();
    expect(onBell1).toHaveBeenCalledTimes(1);
    expect(onBell2).toHaveBeenCalledTimes(1);

    mocks.triggerTitleChange("new title 3");
    expect(onTitle1).toHaveBeenCalledTimes(1);
    expect(onTitle2).toHaveBeenCalledTimes(1);

    terminalHostManager.destroy("sess-events");
  });

  it("unsubscribes previous backendSessionId output subscription when backendSessionId changes in updateSession", async () => {
    const session = createSession("sess-update", "backend-old");
    await terminalHostManager.getOrCreate(session, true);

    mocks.terminalWrites.length = 0;
    mocks.emitSessionOutput("backend-old", "hello old");
    expect(mocks.terminalWrites).toContain("hello old");

    mocks.terminalWrites.length = 0;
    const updatedSession = createSession("sess-update", "backend-new");
    terminalHostManager.updateSession("sess-update", updatedSession);

    // Old backend session output must NOT be written anymore
    mocks.emitSessionOutput("backend-old", "old after update");
    expect(mocks.terminalWrites).not.toContain("old after update");

    // New backend session output MUST be written
    mocks.emitSessionOutput("backend-new", "hello new");
    expect(mocks.terminalWrites).toContain("hello new");

    terminalHostManager.destroy("sess-update");
  });

  it("integrates search addon into terminal instance and disposes it on destroy", async () => {
    const session = createSession("sess-search");
    const instance = await terminalHostManager.getOrCreate(session, true);

    expect(instance.searchAddon).toBeDefined();
    expect(instance.terminal.loadAddon).toHaveBeenCalledWith(instance.searchAddon);

    terminalHostManager.destroy("sess-search");
    expect(mocks.searchAddonDisposed).toHaveBeenCalledTimes(1);
  });
});
