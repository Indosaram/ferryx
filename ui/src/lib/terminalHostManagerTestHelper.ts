import { vi } from "vitest";

let bellHandler: (() => void) | null = null;
let titleHandler: ((title: string) => void) | null = null;
let dataHandler: ((data: string) => void) | null = null;
export const terminalWrites: string[] = [];
export const terminalDisposed = vi.fn();
export const terminalReset = vi.fn();
export const searchAddonDisposed = vi.fn();

export class MockTerminal {
  cols = 80;
  rows = 24;
  unicode = { activeVersion: "6" };
  loadAddon = vi.fn();
  open = vi.fn();
  focus = vi.fn();
  dispose = terminalDisposed;
  reset = vi.fn(() => {
    terminalReset();
  });
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

export class MockFitAddon {
  fit = vi.fn();
}

export class MockSearchAddon {
  dispose = searchAddonDisposed;
  findNext = vi.fn();
  findPrevious = vi.fn();
  clearDecorations = vi.fn();
}

export const outputListeners = new Map<string, Set<(text: string, sequence?: string | null, daemonEpoch?: string | null) => void>>();

export const mocks = {
  MockTerminal,
  MockFitAddon,
  MockSearchAddon,
  terminalWrites,
  terminalDisposed,
  terminalReset,
  searchAddonDisposed,
  outputListeners,
  triggerBell: () => bellHandler?.(),
  triggerTitleChange: (title: string) => titleHandler?.(title),
  triggerData: (data: string) => dataHandler?.(data),
  emitSessionOutput: (sessionId: string, text: string, sequence?: string | null, daemonEpoch?: string | null) => {
    for (const cb of outputListeners.get(sessionId) ?? []) {
      cb(text, sequence, daemonEpoch);
    }
  },
  resetTerminalMocks: () => {
    bellHandler = null;
    titleHandler = null;
    dataHandler = null;
    terminalWrites.length = 0;
    terminalDisposed.mockClear();
    terminalReset.mockClear();
    searchAddonDisposed.mockClear();
    outputListeners.clear();
  },
};

vi.mock("@xterm/xterm", () => ({
  Terminal: MockTerminal,
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: MockFitAddon,
}));

vi.mock("@xterm/addon-search", () => ({
  SearchAddon: MockSearchAddon,
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
    spawnTerminal: vi.fn().mockResolvedValue("mock-spawned-id"),
    closeTerminal: vi.fn().mockResolvedValue(undefined),
    attachTerminal: vi.fn().mockResolvedValue({
      sessionId: "mock-session",
      daemonEpoch: null,
      historyStartSequence: null,
      historyEndSequence: null,
      history: "",
      gap: null,
    }),
  };
});

class MockResizeObserver {
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
}

vi.stubGlobal("ResizeObserver", MockResizeObserver);

import { terminalEventBus } from "./terminalEvents";
import type { TerminalSession } from "./types";

const frameCallbacks = new Map<number, FrameRequestCallback>();
let nextFrameId = 1;

export function flushAnimationFrames(time = performance.now()): void {
  const pending = Array.from(frameCallbacks.values());
  frameCallbacks.clear();
  for (const cb of pending) {
    cb(time);
  }
}

export type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
};

export function createDeferred<T>(): Deferred<T> {
  let resolver: ((value: T) => void) | undefined;
  const promise = new Promise<T>((resolve) => {
    resolver = resolve;
  });
  if (!resolver) {
    throw new Error("Deferred resolver was not initialized");
  }
  return { promise, resolve: resolver };
}

export function createSession(
  id = "sess-1",
  backendSessionId = "backend-1",
  workspaceId = "ws-1",
  daemonEpoch?: string | null,
  lastOutputSequence?: string | null,
): TerminalSession {
  return {
    id,
    cwd: `/workspace/${workspaceId}`,
    workspaceId,
    worktree: { wsId: workspaceId, slug: "main" },
    backendSessionId,
    lifecycle: "working",
    daemonEpoch: daemonEpoch ?? null,
    lastOutputSequence: lastOutputSequence ?? null,
  };
}

export function setupTerminalHostTestEnv(): void {
  mocks.resetTerminalMocks();
  frameCallbacks.clear();
  nextFrameId = 1;
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(() => null);
  vi.stubGlobal("matchMedia", vi.fn(() => ({
    matches: false,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })));
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    const id = nextFrameId++;
    frameCallbacks.set(id, cb);
    return id;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => {
    frameCallbacks.delete(id);
  });
  vi.spyOn(terminalEventBus, "subscribeOutput").mockImplementation((sessionId: string, cb: (text: string, sequence?: string | null, daemonEpoch?: string | null) => void) => {
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
  });
}
