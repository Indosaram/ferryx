import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { NativeTerminalPane } from "./NativeTerminalPane";
import type { TerminalSession } from "../lib/types";

const tauriMock = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriMock.invoke,
  isTauri: tauriMock.isTauri,
}));

type EventHandler = (payload: unknown) => void;

const listeners = new Map<string, Set<EventHandler>>();

const eventMock = vi.hoisted(() => ({ listen: vi.fn() }));

vi.mock("@tauri-apps/api/event", () => ({
  listen: eventMock.listen,
}));

function emitNative(event: string, payload: unknown): void {
  for (const handler of listeners.get(event) ?? []) handler(payload);
}

function session(): TerminalSession {
  return {
    id: "pane-local-id",
    cwd: "/repo/main",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: "backend-session-1",
    lifecycle: "working",
  };
}

describe("NativeTerminalPane surfaces libghostty title and bell events to React", () => {
  beforeEach(() => {
    listeners.clear();
    tauriMock.invoke.mockReset().mockResolvedValue({
      cursorCol: 0,
      cursorRow: 0,
      cellWidthPx: 10,
      cellHeightPx: 20,
    });
    tauriMock.isTauri.mockReset().mockReturnValue(true);
    eventMock.listen.mockReset().mockImplementation((event: string, handler: EventHandler) => {
      const set = listeners.get(event) ?? new Set<EventHandler>();
      set.add(handler);
      listeners.set(event, set);
      return Promise.resolve(() => set.delete(handler));
    });
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("invokes onTitleChange when the native engine reports an OSC title for this session", async () => {
    const onTitleChange = vi.fn();
    render(<NativeTerminalPane session={session()} onTitleChange={onTitleChange} />);

    await waitFor(() => expect(listeners.get("native_terminal_title")?.size).toBeGreaterThan(0));

    emitNative("native_terminal_title", { sessionId: "backend-session-1", title: "my-title" });

    await waitFor(() => expect(onTitleChange).toHaveBeenCalledWith("my-title"));
  });

  it("invokes onBell when the native engine reports a BEL for this session", async () => {
    const onBell = vi.fn();
    render(<NativeTerminalPane session={session()} onBell={onBell} />);

    await waitFor(() => expect(listeners.get("native_terminal_bell")?.size).toBeGreaterThan(0));

    emitNative("native_terminal_bell", { sessionId: "backend-session-1" });

    await waitFor(() => expect(onBell).toHaveBeenCalledTimes(1));
  });

  it("ignores title and bell events addressed to a different backend session", async () => {
    const onTitleChange = vi.fn();
    const onBell = vi.fn();
    render(<NativeTerminalPane session={session()} onTitleChange={onTitleChange} onBell={onBell} />);

    await waitFor(() => expect(listeners.get("native_terminal_title")?.size).toBeGreaterThan(0));

    emitNative("native_terminal_title", { sessionId: "someone-else", title: "not-mine" });
    emitNative("native_terminal_bell", { sessionId: "someone-else" });

    await waitFor(() => expect(tauriMock.invoke).toHaveBeenCalled());
    expect(onTitleChange).not.toHaveBeenCalled();
    expect(onBell).not.toHaveBeenCalled();
  });
});
