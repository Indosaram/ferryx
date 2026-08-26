import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  isTauriRuntime: vi.fn(() => true),
  listTerminalSessions: vi.fn(),
  loadSession: vi.fn(),
  spawnTerminal: vi.fn(),
}));

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return { ...actual, ...tauriMocks };
});

import { createLayoutState } from "./layout";
import type { WorkspaceState } from "./workspaceStore";
import {
  clearHmrWorkspaceState,
  setHmrWorkspaceState,
} from "./hmrWorkspaceState";
import {
  defaultListLiveBackendSessionIds,
  getWorkspaceRestoreStatus,
  resetWorkspaceRestore,
  setWorkspaceRestoreStatus,
  useWorkspaceRestore,
} from "./workspaceRestore";

function persistedSingleTerminal(workspaceId: string, backendSessionId: string, daemonEpoch?: string) {
  return {
    version: 2,
    timestamp: Date.now(),
    activeWorkspaceId: workspaceId,
    workspaces: {
      [workspaceId]: {
        workspaceId,
        repoRoot: "/repo/test",
        worktrees: [{ path: "/repo/test", branch: "main", head: "111", isMain: true, isLocked: false }],
        activeWorktreePath: "/repo/test",
        layout: {
          splitMode: "none" as const,
          primaryTabId: "tab-1",
          secondaryTabId: null,
          activeTabId: "tab-1",
          tabs: [{ id: "tab-1", sessionId: "sess-1", label: "main", kind: "terminal" as const }],
        },
        terminalSessions: {
          "sess-1": {
            localSessionId: "sess-1",
            backendSessionId,
            daemonEpoch,
            lastOutputSequence: daemonEpoch ? "450" : undefined,
            worktreePath: "/repo/test",
            cwd: "/repo/test",
            createdAt: Date.now(),
          },
        },
      },
    },
  };
}

describe("workspaceRestore coordinator", () => {
  beforeEach(() => {
    resetWorkspaceRestore();
    tauriMocks.isTauriRuntime.mockReturnValue(true);
    tauriMocks.listTerminalSessions.mockReset();
    tauriMocks.loadSession.mockReset();
    tauriMocks.spawnTerminal.mockReset();
  });

  it("transitions from idle -> loading -> restored on successful disk restore", async () => {
    const workspaceId = "ws-test";
    const restoreWorkspace = vi.fn();
    let resolveSession!: (session: any) => void;
    const sessionPromise = new Promise((resolve) => {
      resolveSession = resolve;
    });
    const loadSessionFn = vi.fn(() => sessionPromise);
    const listLiveBackendSessionIdsFn = vi.fn(async () => new Set(["live-backend-1"]));

    const { unmount } = renderHook(() =>
      useWorkspaceRestore({
        workspaceId,
        recoveredFromHmr: false,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn,
      }),
    );

    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("loading");
    const sampleSession = persistedSingleTerminal(workspaceId, "live-backend-1");

    await act(async () => {
      resolveSession(sampleSession);
      await sessionPromise;
    });

    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("restored");
    unmount();
  });

  it("does not start restore when enabled is false, and runs restore when enabled becomes true", async () => {
    const workspaceId = "ws-enabled-gate";
    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => persistedSingleTerminal(workspaceId, "backend-1"));
    const listLiveBackendSessionIdsFn = vi.fn(async () => new Set(["backend-1"]));

    const { rerender, unmount } = renderHook(
      ({ enabled }) =>
        useWorkspaceRestore({
          workspaceId,
          recoveredFromHmr: false,
          restoreWorkspace,
          loadSessionFn,
          listLiveBackendSessionIdsFn,
          enabled,
        }),
      { initialProps: { enabled: false } },
    );

    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("idle");
    expect(loadSessionFn).not.toHaveBeenCalled();
    expect(restoreWorkspace).not.toHaveBeenCalled();

    rerender({ enabled: true });

    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("restored");
    expect(loadSessionFn).toHaveBeenCalledTimes(1);
    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    unmount();
  });

  it("safely cancels in-flight restore on StrictMode unmount and allows second mount to complete", async () => {
    const workspaceId = "ws-strictmode";
    const restoreWorkspace = vi.fn();
    let resolveFirstSession!: (session: any) => void;
    const firstSessionPromise = new Promise((resolve) => {
      resolveFirstSession = resolve;
    });
    let resolveSecondSession!: (session: any) => void;
    const secondSessionPromise = new Promise((resolve) => {
      resolveSecondSession = resolve;
    });

    let callCount = 0;
    const loadSessionFn = vi.fn(() => {
      callCount++;
      return callCount === 1 ? firstSessionPromise : secondSessionPromise;
    });
    const listLiveBackendSessionIdsFn = vi.fn(async () => new Set<string>());

    const firstMount = renderHook(() =>
      useWorkspaceRestore({
        workspaceId,
        recoveredFromHmr: false,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn,
      }),
    );
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("loading");
    firstMount.unmount();
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("idle");

    const secondMount = renderHook(() =>
      useWorkspaceRestore({
        workspaceId,
        recoveredFromHmr: false,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn,
      }),
    );
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("loading");
    const sampleSession = persistedSingleTerminal(workspaceId, "missing-backend");

    await act(async () => {
      resolveFirstSession(sampleSession);
      await firstSessionPromise;
    });
    expect(restoreWorkspace).not.toHaveBeenCalled();

    await act(async () => {
      resolveSecondSession(sampleSession);
      await secondSessionPromise;
    });
    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("restored");
    secondMount.unmount();
  });

  it("marks workspace as restored immediately on HMR recovery without disk read when no HMR state is registered", async () => {
    const workspaceId = "ws-hmr";
    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => null);
    const listLiveBackendSessionIdsFn = vi.fn(async () => new Set<string>());
    const { unmount } = renderHook(() =>
      useWorkspaceRestore({
        workspaceId,
        recoveredFromHmr: true,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn,
      }),
    );
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("restored");
    expect(loadSessionFn).not.toHaveBeenCalled();
    expect(restoreWorkspace).not.toHaveBeenCalled();
    unmount();
  });

  it("reconciles stale session to null backendId on HMR recovery when live daemon session list is empty", async () => {
    const workspaceId = "ws-hmr-stale";
    const hmrWorkspaceState: WorkspaceState = {
      worktrees: [{ path: "/repo/test", branch: "main", head: "111", bare: false, detached: false, locked: null, prunable: null }],
      activeWorktreePath: "/repo/test",
      sessions: {
        "sess-stale": {
          id: "sess-stale",
          workspaceId,
          worktree: { wsId: workspaceId, slug: "main" },
          backendSessionId: "dead-backend-pty-999",
          worktreePath: "/repo/test",
          cwd: "/repo/test",
          lifecycle: "working",
        },
      },
      layout: createLayoutState([
        { id: "tab-1", sessionId: "sess-stale", label: "main", kind: "terminal" },
      ]),
      unreadTabIds: {},
      unreadWorktreePaths: {},
    };

    setHmrWorkspaceState(workspaceId, hmrWorkspaceState);
    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => null);
    const listLiveBackendSessionIdsFn = vi.fn(async () => []);

    const { unmount } = renderHook(() =>
      useWorkspaceRestore({
        workspaceId,
        recoveredFromHmr: true,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn,
      }),
    );

    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(loadSessionFn).not.toHaveBeenCalled();
    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    const restored = restoreWorkspace.mock.calls[0][0] as WorkspaceState;
    expect(restored.sessions["sess-stale"]).toMatchObject({
      id: "sess-stale",
      backendSessionId: null,
      lifecycle: "exited",
    });
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("restored");
    clearHmrWorkspaceState(workspaceId);
    unmount();
  });

  it("preserves live backendId on HMR recovery when daemon session remains live", async () => {
    const workspaceId = "ws-hmr-live";
    const hmrWorkspaceState: WorkspaceState = {
      worktrees: [{ path: "/repo/test", branch: "main", head: "111", bare: false, detached: false, locked: null, prunable: null }],
      activeWorktreePath: "/repo/test",
      sessions: {
        "sess-live": {
          id: "sess-live",
          workspaceId,
          worktree: { wsId: workspaceId, slug: "main" },
          backendSessionId: "surviving-backend-pty-123",
          worktreePath: "/repo/test",
          cwd: "/repo/test",
          lifecycle: "working",
        },
      },
      layout: createLayoutState([
        { id: "tab-1", sessionId: "sess-live", label: "main", kind: "terminal" },
      ]),
      unreadTabIds: {},
      unreadWorktreePaths: {},
    };

    setHmrWorkspaceState(workspaceId, hmrWorkspaceState);
    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => null);
    const listLiveBackendSessionIdsFn = vi.fn(async () => [
      { sessionId: "surviving-backend-pty-123" },
    ]);

    const { unmount } = renderHook(() =>
      useWorkspaceRestore({
        workspaceId,
        recoveredFromHmr: true,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn: listLiveBackendSessionIdsFn as any,
      }),
    );

    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(loadSessionFn).not.toHaveBeenCalled();
    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    const restored = restoreWorkspace.mock.calls[0][0] as WorkspaceState;
    expect(restored.sessions["sess-live"]).toMatchObject({
      id: "sess-live",
      backendSessionId: "surviving-backend-pty-123",
      lifecycle: "working",
    });
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("restored");
    clearHmrWorkspaceState(workspaceId);
    unmount();
  });

  it("restores again after the workspace was switched away and back", async () => {
    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => persistedSingleTerminal("project-a", "backend-1"));
    const listLiveBackendSessionIdsFn = vi.fn(async () => [{ sessionId: "backend-1" }]);

    const { rerender } = renderHook(
      ({ workspaceId }: { workspaceId: string }) =>
        useWorkspaceRestore({
          workspaceId,
          recoveredFromHmr: false,
          restoreWorkspace,
          loadSessionFn,
          listLiveBackendSessionIdsFn,
        }),
      { initialProps: { workspaceId: "project-a" } },
    );

    await act(async () => {
      await Promise.resolve();
    });
    expect(getWorkspaceRestoreStatus("project-a")).toBe("restored");
    const callsAfterFirstRestore = loadSessionFn.mock.calls.length;

    rerender({ workspaceId: "project-b" });
    await act(async () => {
      await Promise.resolve();
    });

    rerender({ workspaceId: "project-a" });
    await act(async () => {
      await Promise.resolve();
    });

    expect(loadSessionFn.mock.calls.length).toBeGreaterThan(callsAfterFirstRestore + 1);
  });

  it("isolates restore status per workspace", async () => {
    setWorkspaceRestoreStatus("ws-1", "restored");
    expect(getWorkspaceRestoreStatus("ws-1")).toBe("restored");
    expect(getWorkspaceRestoreStatus("ws-2")).toBe("idle");
    resetWorkspaceRestore("ws-1");
    expect(getWorkspaceRestoreStatus("ws-1")).toBe("idle");
  });

  it("transitions to failed status if loadSession throws an unhandled error", async () => {
    const workspaceId = "ws-fail";
    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => {
      throw new Error("Disk read corrupted");
    });
    const listLiveBackendSessionIdsFn = vi.fn(async () => new Set<string>());
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { unmount } = renderHook(() =>
      useWorkspaceRestore({
        workspaceId,
        recoveredFromHmr: false,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn,
      }),
    );
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("failed");
    expect(restoreWorkspace).not.toHaveBeenCalled();
    warnSpy.mockRestore();
    unmount();
  });

  it("propagates daemon list failure instead of treating it as an empty live-session set", async () => {
    tauriMocks.listTerminalSessions.mockRejectedValueOnce(new Error("daemon list unavailable"));
    await expect(defaultListLiveBackendSessionIds()).rejects.toThrow("daemon list unavailable");
  });

  it("keeps persisted backend mappings untouched and fails restore when daemon listing fails", async () => {
    const workspaceId = "ws-list-fail";
    const persisted = persistedSingleTerminal(workspaceId, "backend-still-live", "epoch-500");
    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => persisted);
    tauriMocks.listTerminalSessions.mockRejectedValueOnce(new Error("daemon unavailable"));
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});

    const { unmount } = renderHook(() =>
      useWorkspaceRestore({
        workspaceId,
        recoveredFromHmr: false,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn: defaultListLiveBackendSessionIds,
      }),
    );

    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("failed");
    expect(restoreWorkspace).not.toHaveBeenCalled();
    expect(tauriMocks.spawnTerminal).not.toHaveBeenCalled();
    expect(persisted.workspaces[workspaceId].terminalSessions["sess-1"].backendSessionId).toBe("backend-still-live");
    expect(persisted.workspaces[workspaceId].terminalSessions["sess-1"].daemonEpoch).toBe("epoch-500");

    warnSpy.mockRestore();
    unmount();
  });

  it("reconciles sessions with daemon epoch and backend ID matching from live session query", async () => {
    const workspaceId = "ws-epoch-match";
    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => persistedSingleTerminal(workspaceId, "backend-1", "epoch-100"));
    const listLiveBackendSessionIdsFn = vi.fn(async () => [
      { sessionId: "backend-1", daemonEpoch: "epoch-100" },
    ]);
    const { unmount } = renderHook(() =>
      useWorkspaceRestore({
        workspaceId,
        recoveredFromHmr: false,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn: listLiveBackendSessionIdsFn as any,
      }),
    );
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    const restored = restoreWorkspace.mock.calls[0][0] as WorkspaceState;
    expect(restored.sessions["sess-1"]).toMatchObject({
      id: "sess-1",
      backendSessionId: "backend-1",
      lifecycle: "working",
      daemonEpoch: "epoch-100",
      // Cold restore starts with an empty terminal; replay must not be suppressed.
      lastOutputSequence: null,
    });
    unmount();
  });

  it("marks session as exited without auto-respawn when daemon epoch has changed", async () => {
    const workspaceId = "ws-epoch-mismatch";
    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => persistedSingleTerminal(workspaceId, "backend-1", "epoch-OLD"));
    const listLiveBackendSessionIdsFn = vi.fn(async () => [
      { sessionId: "backend-1", daemonEpoch: "epoch-NEW" },
    ]);
    const { unmount } = renderHook(() =>
      useWorkspaceRestore({
        workspaceId,
        recoveredFromHmr: false,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn: listLiveBackendSessionIdsFn as any,
      }),
    );
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    const restored = restoreWorkspace.mock.calls[0][0] as WorkspaceState;
    expect(restored.sessions["sess-1"]).toMatchObject({
      id: "sess-1",
      backendSessionId: null,
      lifecycle: "exited",
      daemonEpoch: null,
      lastOutputSequence: null,
    });
    unmount();
  });
});