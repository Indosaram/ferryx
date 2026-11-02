import { JSDOM } from "jsdom";

if (typeof window === "undefined") {
  const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>", { url: "http://localhost:3000" });
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  globalThis.navigator = dom.window.navigator;
  globalThis.localStorage = dom.window.localStorage;
  globalThis.sessionStorage = dom.window.sessionStorage;
  globalThis.HTMLElement = dom.window.HTMLElement;
}

import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { clearHmrWorkspaceState } from "./hmrWorkspaceState";
import { clearWorkspaceSnapshot } from "./workspaceSnapshotCache";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const services: WorkspaceServices = {
  ensureTerminalEvents: vi.fn(async () => undefined),
  spawnTerminal: vi.fn(async () => "backend-spawned"),
  getTerminalCwd: vi.fn(async () => null),
  closeTerminal: vi.fn(async () => undefined),
  waitForTerminalExit: vi.fn(async () => undefined),
};

const worktree: Worktree = {
  path: "/repo/main", head: "abc", branch: "refs/heads/main", workspaceId: "ws-move",
  bare: false, detached: false, locked: null, prunable: null,
};

describe("workspaceStore window moves", () => {
  afterEach(() => {
    vi.clearAllMocks();
    clearHmrWorkspaceState();
    clearWorkspaceSnapshot();
  });

  it("adopts a moved backend session without spawning and seeds the attach generation floor", async () => {
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId: "ws-move", initialWorktrees: [worktree], services }));
    let tabId: string | null = null;
    await act(async () => {
      tabId = await result.current.adoptBackendSession(
        worktree,
        { sessionId: "backend-moved", daemonEpoch: "7", incarnation: "inc-1", attemptGeneration: 4 },
        undefined,
        { activate: true },
      );
    });
    expect(tabId).not.toBeNull();
    expect(services.spawnTerminal).not.toHaveBeenCalled();
    const session = Object.values(result.current.state.sessions).find((s) => s.backendSessionId === "backend-moved");
    expect(session?.attachTuple).toMatchObject({ backendSessionId: "backend-moved", daemonEpoch: "7", attemptGeneration: 4 });
    expect(result.current.state.layout.activeTabId).toBe(tabId);
  });

  it("releaseTabsForMove drops the tab and its session but never closes the backend session", async () => {
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId: "ws-move", initialWorktrees: [worktree], services }));
    let tabId: string | null = null;
    await act(async () => {
      tabId = await result.current.adoptBackendSession(worktree, { sessionId: "backend-moved", daemonEpoch: "7", incarnation: "inc-1" });
    });
    expect(result.current.state.layout.tabs.some((tab) => tab.id === tabId)).toBe(true);

    await act(async () => {
      result.current.releaseTabsForMove([tabId as unknown as string]);
    });
    expect(result.current.state.layout.tabs.some((tab) => tab.id === tabId)).toBe(false);
    expect(Object.values(result.current.state.sessions).some((s) => s.backendSessionId === "backend-moved")).toBe(false);
    expect(services.closeTerminal).not.toHaveBeenCalled();
  });
});
