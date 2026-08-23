import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { WorkspaceState } from "./workspaceStore";
import {
  getWorkspaceRestoreStatus,
  resetWorkspaceRestore,
  setWorkspaceRestoreStatus,
  useWorkspaceRestore,
} from "./workspaceRestore";

describe("workspaceRestore coordinator", () => {
  beforeEach(() => {
    resetWorkspaceRestore();
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

    const sampleSession = {
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
              backendSessionId: "live-backend-1",
              worktreePath: "/repo/test",
              cwd: "/repo/test",
              createdAt: Date.now(),
            },
          },
        },
      },
    };

    await act(async () => {
      resolveSession(sampleSession);
      await sessionPromise;
    });

    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("restored");

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

    // Mount 1 (StrictMode first pass)
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

    // Cleanup 1 (StrictMode unmounts first pass)
    firstMount.unmount();
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("idle");

    // Mount 2 (StrictMode second pass)
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

    const sampleSession = {
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
              backendSessionId: "missing-backend",
              worktreePath: "/repo/test",
              cwd: "/repo/test",
              createdAt: Date.now(),
            },
          },
        },
      },
    };

    // Resolving first promise after unmount must be ignored by mount 1
    await act(async () => {
      resolveFirstSession(sampleSession);
      await firstSessionPromise;
    });
    expect(restoreWorkspace).not.toHaveBeenCalled();

    // Resolving second promise completes mount 2
    await act(async () => {
      resolveSecondSession(sampleSession);
      await secondSessionPromise;
    });

    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("restored");

    secondMount.unmount();
  });

  it("marks workspace as restored immediately on HMR recovery without disk read", async () => {
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

  it("reconciles sessions with daemon epoch and backend ID matching from live session query", async () => {
    const workspaceId = "ws-epoch-match";
    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => ({
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
              backendSessionId: "backend-1",
              daemonEpoch: "epoch-100",
              lastOutputSequence: "450",
              worktreePath: "/repo/test",
              cwd: "/repo/test",
              createdAt: Date.now(),
            },
          },
        },
      },
    }));

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
      lastOutputSequence: "450",
    });

    unmount();
  });

  it("marks session as exited without auto-respawn when daemon epoch has changed", async () => {
    const workspaceId = "ws-epoch-mismatch";
    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => ({
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
              backendSessionId: "backend-1",
              daemonEpoch: "epoch-OLD",
              lastOutputSequence: "450",
              worktreePath: "/repo/test",
              cwd: "/repo/test",
              createdAt: Date.now(),
            },
          },
        },
      },
    }));

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
