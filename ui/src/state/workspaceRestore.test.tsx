import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  isTauriRuntime: vi.fn(() => true),
  listTerminalSessions: vi.fn(),
  loadSession: vi.fn(),
  spawnTerminal: vi.fn(),
  describeTerminal: vi.fn(),
}));

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return { ...actual, ...tauriMocks };
});

import { createLayoutState } from "./layout";
import {
  useWorkspaceStore,
  setLocalRecoveryPersistence,
  wakeLocalSessionRecovery,
  requestLocalRecoveryAdmission,
  type LocalRecoveryAdmissionResult,
  type WorkspaceServices,
  type WorkspaceState,
} from "./workspaceStore";
import {
  clearHmrWorkspaceState,
  getHmrWorkspaceState,
  setHmrWorkspaceState,
} from "./hmrWorkspaceState";
import {
  defaultListLiveBackendSessionIds,
  getWorkspaceRestoreStatus,
  preloadWorkspaceSnapshots,
  resetWorkspaceRestore,
  setWorkspaceRestoreStatus,
  useWorkspaceRestore,
} from "./workspaceRestore";
import {
  clearWorkspaceSnapshot,
  getWorkspaceSnapshot,
  setWorkspaceSnapshot,
  subscribeWorkspaceSnapshots,
} from "./workspaceSnapshotCache";

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
    clearWorkspaceSnapshot();
    clearHmrWorkspaceState();
    tauriMocks.isTauriRuntime.mockReturnValue(true);
    tauriMocks.listTerminalSessions.mockReset();
    tauriMocks.loadSession.mockReset();
    tauriMocks.spawnTerminal.mockReset();
  });

  it("preloads mixed local and paired layouts while the local daemon is unavailable", async () => {
    const pairedId = `daemon:${"a".repeat(64)}`;
    const local = persistedSingleTerminal("local", "local-proxy");
    const paired = persistedSingleTerminal(pairedId, "paired-proxy", "42");
    const persisted = { ...local, workspaces: { ...local.workspaces, [pairedId]: {
      ...paired.workspaces[pairedId], target: { kind: "pairedDaemon" as const, hostId: "host-a" }, remoteWorkspaceId: "remote-root",
    } } };
    await preloadWorkspaceSnapshots(["local", pairedId], async () => persisted, async () => { throw new Error("offline"); });
    expect(getWorkspaceSnapshot(pairedId)?.sessions["sess-1"]).toMatchObject({ backendSessionId: "paired-proxy", remoteConnectionState: "reconnecting", lastOutputSequence: null });
    expect(getWorkspaceSnapshot("local")?.layout.tabs[0].id).toBe("tab-1");
    expect(tauriMocks.spawnTerminal).not.toHaveBeenCalled();
  });

  it("preserves newer HMR state and does not restore stale preloaded snapshot when recoveredFromHmr is true", async () => {
    const workspaceId = "ws-returned-hmr";
    const persisted = persistedSingleTerminal(workspaceId, "backend-1");

    await preloadWorkspaceSnapshots(
      [workspaceId],
      async () => persisted,
      async () => [{ sessionId: "backend-1" }],
    );

    const hmr2TabState: WorkspaceState = {
      workspaceId,
      worktrees: [{ path: "/repo/test", branch: "main", head: "111", bare: false, detached: false, locked: null, prunable: null }],
      activeWorktreePath: "/repo/test",
      sessions: {
        "sess-1": {
          id: "sess-1",
          workspaceId,
          worktree: { wsId: workspaceId, slug: "main" },
          backendSessionId: "backend-1",
          worktreePath: "/repo/test",
          cwd: "/repo/test",
          lifecycle: "working",
        },
        "sess-2": {
          id: "sess-2",
          workspaceId,
          worktree: { wsId: workspaceId, slug: "main" },
          backendSessionId: "backend-2",
          worktreePath: "/repo/test",
          cwd: "/repo/test",
          lifecycle: "working",
        },
      },
      layout: createLayoutState([
        { id: "tab-1", sessionId: "sess-1", label: "main", kind: "terminal" },
        { id: "tab-2", sessionId: "sess-2", label: "tab-2", kind: "terminal" },
      ]),
      unreadTabIds: {},
      unreadWorktreePaths: {},
    };

    setHmrWorkspaceState(workspaceId, hmr2TabState);

    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => {
      throw new Error("preloaded HMR recovery must not trigger disk read");
    });

    const { unmount } = renderHook(() =>
      useWorkspaceRestore({
        workspaceId,
        recoveredFromHmr: true,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn: async () => [
          { sessionId: "backend-1" },
          { sessionId: "backend-2" },
        ],
      }),
    );

    expect(restoreWorkspace).not.toHaveBeenCalled();
    expect(loadSessionFn).not.toHaveBeenCalled();
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("restored");
    expect(getHmrWorkspaceState(workspaceId)?.layout.tabs).toHaveLength(2);
    clearHmrWorkspaceState(workspaceId);
    unmount();
  });

  it("preloads every persisted workspace snapshot before project switching", async () => {
    const alpha = persistedSingleTerminal("alpha", "backend-alpha");
    const beta = persistedSingleTerminal("beta", "backend-beta");
    const persisted = {
      ...alpha,
      workspaces: {
        ...alpha.workspaces,
        ...beta.workspaces,
      },
    };

    await preloadWorkspaceSnapshots(
      ["alpha", "beta"],
      async () => persisted,
      async () => [
        { sessionId: "backend-alpha" },
        { sessionId: "backend-beta" },
      ],
    );

    expect(getWorkspaceSnapshot("alpha")).toMatchObject({
      workspaceId: "alpha",
      layout: { tabs: [{ id: "tab-1" }] },
    });
    expect(getWorkspaceSnapshot("beta")).toMatchObject({
      workspaceId: "beta",
      layout: { tabs: [{ id: "tab-1" }] },
    });

    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => {
      throw new Error("preloaded state must avoid a second disk read");
    });
    const { unmount } = renderHook(() =>
      useWorkspaceRestore({
        workspaceId: "beta",
        recoveredFromHmr: false,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn: async () => [],
      }),
    );

    expect(restoreWorkspace).toHaveBeenCalledWith(
      expect.objectContaining({
        workspaceId: "beta",
        layout: expect.objectContaining({
          tabs: [expect.objectContaining({ id: "tab-1" })],
        }),
      }),
    );
    expect(loadSessionFn).not.toHaveBeenCalled();
    expect(getWorkspaceRestoreStatus("beta")).toBe("restored");
    unmount();
  });

  it("exposes preloaded tabs on the first render of a project switch", async () => {
    const alpha = persistedSingleTerminal("alpha", "backend-alpha");
    const beta = persistedSingleTerminal("beta", "backend-beta");
    await preloadWorkspaceSnapshots(
      ["alpha", "beta"],
      async () => ({
        ...alpha,
        workspaces: {
          ...alpha.workspaces,
          ...beta.workspaces,
        },
      }),
      async () => [
        { sessionId: "backend-alpha" },
        { sessionId: "backend-beta" },
      ],
    );
    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(async () => "unexpected-spawn"),
      getTerminalCwd: vi.fn(async () => null),
      closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };
    const observed: Array<{
      requestedWorkspaceId: string;
      stateWorkspaceId: string | undefined;
      tabCount: number;
    }> = [];
    const { rerender } = renderHook(
      ({ workspaceId }: { workspaceId: string }) => {
        const store = useWorkspaceStore({ workspaceId, services });
        observed.push({
          requestedWorkspaceId: workspaceId,
          stateWorkspaceId: store.state.workspaceId,
          tabCount: store.state.layout.tabs.length,
        });
        return store;
      },
      { initialProps: { workspaceId: "alpha" } },
    );

    observed.length = 0;
    rerender({ workspaceId: "beta" });

    expect(observed).not.toHaveLength(0);
    expect(observed).toEqual(
      observed.map(() => ({
        requestedWorkspaceId: "beta",
        stateWorkspaceId: "beta",
        tabCount: 1,
      })),
    );
    expect(services.spawnTerminal).not.toHaveBeenCalled();
  });

  it("restores the live snapshot, not the boot-time preload, when registration completes late", async () => {
    const workspaceId = "ws-late-registration";
    await preloadWorkspaceSnapshots(
      [workspaceId],
      async () => persistedSingleTerminal(workspaceId, "backend-boot"),
      async () => [{ sessionId: "backend-boot" }],
    );
    const preloaded = getWorkspaceSnapshot(workspaceId)!;
    // While the project stays unregistered the user keeps working: panes are replaced and
    // the mounted store publishes each change to the snapshot cache.
    const live: WorkspaceState = {
      ...preloaded,
      sessions: {
        "sess-new": { ...preloaded.sessions["sess-1"], id: "sess-new", backendSessionId: "backend-new" },
      },
      layout: createLayoutState([{ id: "tab-new", sessionId: "sess-new", label: "main", kind: "terminal" }]),
    };
    setWorkspaceSnapshot(workspaceId, live);

    const restoreWorkspace = vi.fn();
    const { rerender, unmount } = renderHook(
      ({ enabled }) =>
        useWorkspaceRestore({
          workspaceId,
          recoveredFromHmr: false,
          restoreWorkspace,
          loadSessionFn: async () => {
            throw new Error("preloaded restore must not read disk");
          },
          listLiveBackendSessionIdsFn: async () => [],
          enabled,
        }),
      { initialProps: { enabled: false } },
    );
    expect(restoreWorkspace).not.toHaveBeenCalled();

    rerender({ enabled: true });

    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    const restored = restoreWorkspace.mock.calls[0][0] as WorkspaceState;
    expect(restored.layout.tabs.map((tab) => tab.id)).toEqual(["tab-new"]);
    expect(Object.keys(restored.sessions)).toEqual(["sess-new"]);
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("restored");
    unmount();
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

  it("retains unknown HMR identity when live daemon session list is empty", async () => {
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
    let restoredSignal!: () => void;
    const restoredPromise = new Promise<void>((resolve) => { restoredSignal = resolve; });
    const restoreWorkspace = vi.fn((_state: WorkspaceState) => restoredSignal());
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

    await act(async () => { await restoredPromise; });

    expect(loadSessionFn).not.toHaveBeenCalled();
    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    const restored = restoreWorkspace.mock.calls[0][0] as WorkspaceState;
    expect(restored.sessions["sess-stale"]).toMatchObject({
      id: "sess-stale",
      backendSessionId: "dead-backend-pty-999",
      lifecycle: "working",
      remoteConnectionState: "reconnecting",
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
  it("rejects persisted-session load failure before attempting inventory", async () => {
    const inventory = vi.fn(async () => []);
    await expect(preloadWorkspaceSnapshots(["local"], async () => { throw new Error("session unreadable"); }, inventory))
      .rejects.toThrow("session unreadable");
    expect(inventory).not.toHaveBeenCalled();
    expect(getWorkspaceSnapshot("local")).toBeNull();
  });

  it.each([
    { incarnation: "original-pty", backendSessionId: "backend-1", daemonEpoch: "18446744073709551614", lifecycle: "working" },
    { incarnation: "replacement-pty", backendSessionId: "backend-1", daemonEpoch: "18446744073709551613", lifecycle: "working" },
  ])("preloads default inventory across epochs with $incarnation", async ({ incarnation, backendSessionId, daemonEpoch, lifecycle }) => {
    const workspaceId = "ws-default-inventory";
    const persisted = persistedSingleTerminal(workspaceId, "backend-1", "18446744073709551613");
    const session = persisted.workspaces[workspaceId].terminalSessions["sess-1"];
    tauriMocks.loadSession.mockResolvedValueOnce({
      ...persisted,
      workspaces: {
        [workspaceId]: {
          ...persisted.workspaces[workspaceId],
          terminalSessions: { "sess-1": { ...session, incarnation: "original-pty" } },
        },
      },
    });
    tauriMocks.listTerminalSessions.mockResolvedValueOnce([{
      sessionId: "backend-1",
      worktreePath: "/repo/test",
      running: true,
      daemonEpoch: "18446744073709551614",
      incarnation,
    }]);

    await preloadWorkspaceSnapshots([workspaceId]);

    expect(tauriMocks.loadSession).toHaveBeenCalledTimes(1);
    expect(tauriMocks.listTerminalSessions).toHaveBeenCalledTimes(1);
    expect(getWorkspaceSnapshot(workspaceId)?.sessions["sess-1"]).toMatchObject({
      id: "sess-1",
      backendSessionId,
      daemonEpoch,
      lifecycle,
      incarnation: "original-pty",
      worktreePath: "/repo/test",
      lastOutputSequence: null,
    });
    expect(tauriMocks.spawnTerminal).not.toHaveBeenCalled();
  });

  it("restores SSH pane identity while the daemon list is temporarily unavailable", async () => {
    const workspaceId = "ssh:list-unavailable";
    const persisted = persistedSingleTerminal(workspaceId, "stable", "old");
    let complete!: () => void;
    const restored = new Promise<void>(resolve => { complete = resolve; });
    const restoreWorkspace = vi.fn((_state: WorkspaceState) => complete());
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const loadSessionFn = async () => persisted;
    const listLiveBackendSessionIdsFn = async () => { throw new Error("temporarily unavailable"); };
    const hook = renderHook(() => useWorkspaceRestore({ workspaceId, recoveredFromHmr: false,
      restoreWorkspace, loadSessionFn, listLiveBackendSessionIdsFn,
    }));
    await act(async () => { await restored; });
    expect(restoreWorkspace.mock.calls[0][0].sessions["sess-1"].backendSessionId).toBe("stable");
    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("restored");
    hook.unmount();
    warn.mockRestore();
  }, 1000);

  it("restores retained local identity when daemon listing fails", async () => {
    const workspaceId = "ws-list-fail";
    const persisted = persistedSingleTerminal(workspaceId, "backend-still-live", "epoch-500");
    let restoredSignal!: () => void;
    const restoredPromise = new Promise<void>((resolve) => { restoredSignal = resolve; });
    const restoreWorkspace = vi.fn((_state: WorkspaceState) => restoredSignal());
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

    await act(async () => { await restoredPromise; });

    expect(getWorkspaceRestoreStatus(workspaceId)).toBe("restored");
    expect(restoreWorkspace.mock.calls[0][0].sessions["sess-1"]).toMatchObject({
      backendSessionId: "backend-still-live", daemonEpoch: "epoch-500", remoteConnectionState: "reconnecting",
    });
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

  it("retains original identity and blocks recovery without auto-respawn on inventory incarnation conflict", async () => {
    const workspaceId = "ws-epoch-mismatch";
    let restoredSignal!: () => void;
    const restoredPromise = new Promise<void>((resolve) => { restoredSignal = resolve; });
    const restoreWorkspace = vi.fn((_state: WorkspaceState) => restoredSignal());
    const persisted = persistedSingleTerminal(workspaceId, "backend-1", "epoch-OLD");
    const loadSessionFn = vi.fn(async () => ({ ...persisted, workspaces: {
      ...persisted.workspaces, [workspaceId]: { ...persisted.workspaces[workspaceId], terminalSessions: {
        "sess-1": { ...persisted.workspaces[workspaceId].terminalSessions["sess-1"], incarnation: "old-pty" },
      } },
    } }));
    const listLiveBackendSessionIdsFn = vi.fn(async () => [
      { sessionId: "backend-1", daemonEpoch: "epoch-NEW", incarnation: "replacement-pty" },
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
    await act(async () => { await restoredPromise; });
    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    const restored = restoreWorkspace.mock.calls[0][0] as WorkspaceState;
    expect(restored.sessions["sess-1"]).toMatchObject({
      id: "sess-1",
      backendSessionId: "backend-1",
      lifecycle: "working",
      remoteConnectionState: "disconnected",
      reconnectLifecycle: "failed",
      reconnectError: { code: "IDENTITY_CONFLICT" },
      backendUnavailableReason: "identity_conflict",
      incarnation: "old-pty",
      daemonEpoch: "epoch-OLD",
      lastOutputSequence: null,
    });
    expect(tauriMocks.spawnTerminal).not.toHaveBeenCalled();
    unmount();
  });

  it("preserves live backend session identity, agent metadata, and tab ownership without calling spawnTerminal", async () => {
    const workspaceId = "ws-warm-live";
    const persisted = {
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
            tabs: [
              {
                id: "tab-1",
                sessionId: "sess-1",
                label: "agent-tab",
                kind: "terminal" as const,
                terminal: {
                  paneTree: {
                    type: "split" as const,
                    direction: "horizontal" as const,
                    first: { type: "leaf" as const, leafId: "leaf-1" },
                    second: { type: "leaf" as const, leafId: "leaf-2" },
                    ratio: 0.5,
                  },
                  activeLeafId: "leaf-2",
                  expandedLeafId: null,
                  sessionIdsByLeafId: {
                    "leaf-1": "sess-1",
                    "leaf-2": "sess-2",
                  },
                },
              },
            ],
          },
          terminalSessions: {
            "sess-1": {
              localSessionId: "sess-1",
              backendSessionId: "backend-live-1",
              daemonEpoch: "epoch-live",
              lastOutputSequence: "350",
              worktreePath: "/repo/test",
              cwd: "/repo/test/packages/core",
              agentType: "claude",
              agentSessionId: "claude-session-uuid-1",
              createdAt: Date.now(),
            },
            "sess-2": {
              localSessionId: "sess-2",
              backendSessionId: "backend-live-2",
              daemonEpoch: "epoch-live",
              lastOutputSequence: "720",
              worktreePath: "/repo/test",
              cwd: "/repo/test/packages/cli",
              agentType: "omo",
              agentSessionId: "omo-session-uuid-2",
              createdAt: Date.now(),
            },
          },
        },
      },
    };

    const restoreWorkspace = vi.fn();
    const loadSessionFn = vi.fn(async () => persisted);
    const listLiveBackendSessionIdsFn = vi.fn(async () => [
      { sessionId: "backend-live-1", daemonEpoch: "epoch-live" },
      { sessionId: "backend-live-2", daemonEpoch: "epoch-live" },
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
      backendSessionId: "backend-live-1",
      lifecycle: "working",
      daemonEpoch: "epoch-live",
      cwd: "/repo/test/packages/core",
      agentType: "claude",
      agentSessionId: "claude-session-uuid-1",
    });

    expect(restored.sessions["sess-2"]).toMatchObject({
      id: "sess-2",
      backendSessionId: "backend-live-2",
      lifecycle: "working",
      daemonEpoch: "epoch-live",
      cwd: "/repo/test/packages/cli",
      agentType: "omo",
      agentSessionId: "omo-session-uuid-2",
    });

    const tabLayout = restored.layout.layoutsByTabId["tab-1"];
    expect(tabLayout).toBeDefined();
    expect(tabLayout.sessionIdsByLeafId).toEqual({
      "leaf-1": "sess-1",
      "leaf-2": "sess-2",
    });

    expect(tauriMocks.spawnTerminal).not.toHaveBeenCalled();
    unmount();
  });

  it("under Active Only policy, retains both unknown backend identities without standby replacement", async () => {
    localStorage.setItem(
      "ferryx.settings.general",
      JSON.stringify({ confirmCloseTab: false, sessionRestorePolicy: "activeOnly" }),
    );

    const workspaceId = "ws-active-only";
    const persisted = {
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
            tabs: [
              {
                id: "tab-1",
                sessionId: "sess-1",
                label: "active-tab",
                kind: "terminal" as const,
              },
              {
                id: "tab-2",
                sessionId: "sess-2",
                label: "inactive-tab",
                kind: "terminal" as const,
              },
            ],
          },
          terminalSessions: {
            "sess-1": {
              localSessionId: "sess-1",
              backendSessionId: "backend-1",
              cwd: "/repo/test",
              worktreePath: "/repo/test",
              lifecycle: "exited" as const,
            },
            "sess-2": {
              localSessionId: "sess-2",
              backendSessionId: "backend-2",
              cwd: "/repo/test",
              worktreePath: "/repo/test",
              lifecycle: "exited" as const,
            },
          },
        },
      },
    };

    let restoredSignal!: () => void;
    const restoredPromise = new Promise<void>((resolve) => { restoredSignal = resolve; });
    const restoreWorkspace = vi.fn((_state: WorkspaceState) => restoredSignal());
    const loadSessionFn = vi.fn(async () => persisted);
    const listLiveBackendSessionIdsFn = vi.fn(async () => []);

    const { unmount } = renderHook(() =>
      useWorkspaceRestore({
        workspaceId,
        recoveredFromHmr: false,
        restoreWorkspace,
        loadSessionFn,
        listLiveBackendSessionIdsFn: listLiveBackendSessionIdsFn as any,
      }),
    );

    await act(async () => { await restoredPromise; });

    expect(restoreWorkspace).toHaveBeenCalledTimes(1);
    const restored = restoreWorkspace.mock.calls[0][0] as WorkspaceState;

    expect(restored.sessions["sess-1"]?.backendSessionId).toBe("backend-1");
    expect(restored.sessions["sess-2"]?.backendSessionId).toBe("backend-2");
    expect(restored.sessions["sess-1"]?.remoteConnectionState).toBe("reconnecting");
    expect(restored.sessions["sess-2"]?.remoteConnectionState).toBe("reconnecting");

    localStorage.removeItem("ferryx.settings.general");
    unmount();
  });
});

describe("retained local recovery owner", () => {
  function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>((done) => { resolve = done; });
    return { promise, resolve };
  }
  async function load() {
    const persisted = persistedSingleTerminal("local", "backend", "old");
    Object.assign(persisted.workspaces.local.terminalSessions["sess-1"], { incarnation: "process" });
    await preloadWorkspaceSnapshots(["local"], async () => persisted, async () => { throw new Error("offline"); });
    return getWorkspaceSnapshot("local")!;
  }
  const running = () => ({ sessionId: "backend", incarnation: "process", daemonEpoch: "new", running: true,
    recoveryAuthority: { status: "running", incarnation: "process", daemonEpoch: "new" } });
  beforeEach(() => {
    resetWorkspaceRestore(); clearWorkspaceSnapshot(); clearHmrWorkspaceState(); vi.useFakeTimers();
    tauriMocks.isTauriRuntime.mockReturnValue(true);
    tauriMocks.describeTerminal.mockReset();
  });
  afterEach(() => {
    cleanup(); clearWorkspaceSnapshot(); setLocalRecoveryPersistence(undefined); vi.useRealTimers();
  });
  it("hands startup identities off without a competing inventory timer", async () => {
    await load();
    const registered = deferred<void>();
    const registration = vi.fn(() => registered.resolve());
    const list = vi.fn();
    const hook = renderHook(() => useWorkspaceRestore({ workspaceId: "local", recoveredFromHmr: false,
      restoreWorkspace: vi.fn(), reconcileLocalSessions: registration, listLiveBackendSessionIdsFn: list }));
    await act(async () => { await registered.promise; });
    expect(registration).toHaveBeenCalledWith();
    expect(list).not.toHaveBeenCalled();
    hook.unmount();
  });
  it("holds both requesters pending through rejected saves and admits only the saved publication", async () => {
    await load();
    tauriMocks.describeTerminal.mockResolvedValue(running());
    const saveStarted = deferred<void>();
    const save = deferred<void>();
    const ready = deferred<void>();
    const first: LocalRecoveryAdmissionResult[] = [];
    const second: LocalRecoveryAdmissionResult[] = [];
    setLocalRecoveryPersistence(vi.fn().mockRejectedValueOnce(new Error("save rejected")).mockImplementation(() => {
      saveStarted.resolve(); return save.promise;
    }));
    const unsubscribe = requestLocalRecoveryAdmission("local", "sess-1", true, result => first.push(result));
    requestLocalRecoveryAdmission("local", "sess-1", true, result => {
      second.push(result); if (result.kind === "ready") ready.resolve();
    });
    await act(async () => { await vi.advanceTimersByTimeAsync(4000); await saveStarted.promise; });
    expect(first).toEqual([{ kind: "pending" }]);
    expect(second).toEqual([{ kind: "pending" }]);
    unsubscribe();
    await act(async () => { save.resolve(); await ready.promise; });
    expect(first).toEqual([{ kind: "pending" }]);
    expect(second[1]).toEqual({ kind: "ready", session: getWorkspaceSnapshot("local")!.sessions["sess-1"] });
    const admitted: LocalRecoveryAdmissionResult[] = [];
    requestLocalRecoveryAdmission("local", "sess-1", false, result => admitted.push(result));
    expect(admitted[0].kind).toBe("ready");
    wakeLocalSessionRecovery("local", "sess-1");
    expect(getWorkspaceSnapshot("local")!.sessions["sess-1"].remoteConnectionState).not.toBe("reconnecting");
  });
  it("cancels a requester on external token replacement and never attaches a conflict", async () => {
    await load();
    const describe = deferred<ReturnType<typeof running>>();
    tauriMocks.describeTerminal.mockReturnValue(describe.promise);
    setLocalRecoveryPersistence(async () => {});
    const outcomes: LocalRecoveryAdmissionResult[] = [];
    requestLocalRecoveryAdmission("local", "sess-1", true, result => outcomes.push(result));
    const current = getWorkspaceSnapshot("local")!;
    setWorkspaceSnapshot("local", { ...current, sessions: { ...current.sessions,
      "sess-1": { ...current.sessions["sess-1"], recoveryToken: 999, backendUnavailableReason: "identity_conflict" } } });
    expect(outcomes).toEqual([{ kind: "pending" }, { kind: "cancelled" }]);

    // Passive admission (false) returns blocked synchronously with no extra probe:
    const countBeforePassive = tauriMocks.describeTerminal.mock.calls.length;
    const passiveBlocked: LocalRecoveryAdmissionResult[] = [];
    requestLocalRecoveryAdmission("local", "sess-1", false, result => passiveBlocked.push(result));
    expect(passiveBlocked).toEqual([{ kind: "blocked", reason: "identity_conflict" }]);
    expect(tauriMocks.describeTerminal).toHaveBeenCalledTimes(countBeforePassive);

    wakeLocalSessionRecovery("local", "sess-1");
    expect(tauriMocks.describeTerminal).toHaveBeenCalledTimes(countBeforePassive);

    // Subscribe exact cache completion before stale probe resolves to verify it cannot admit:
    let staleProbeAttemptedAdmission = false;
    const unsubscribeCache = subscribeWorkspaceSnapshots(() => {
      const snap = getWorkspaceSnapshot("local");
      if (snap?.sessions["sess-1"]?.remoteConnectionState === "reconnecting" ||
          snap?.sessions["sess-1"]?.backendUnavailableReason !== "identity_conflict") {
        staleProbeAttemptedAdmission = true;
      }
    });
    describe.resolve(running());

    // Explicit manual retry (true) is pending by design, probes authority, and stays blocked on held mismatch:
    const revalDescribe = deferred<ReturnType<typeof running>>();
    tauriMocks.describeTerminal.mockReturnValueOnce(revalDescribe.promise);
    const explicitCompleted = deferred<LocalRecoveryAdmissionResult>();
    const explicitResults: LocalRecoveryAdmissionResult[] = [];
    const unsubscribeExplicit = requestLocalRecoveryAdmission("local", "sess-1", true, result => {
      explicitResults.push(result);
      if (result.kind !== "pending") explicitCompleted.resolve(result);
    });
    expect(explicitResults).toEqual([{ kind: "pending" }]);
    expect(tauriMocks.describeTerminal).toHaveBeenCalledTimes(countBeforePassive + 1);

    revalDescribe.resolve({ ...running(), recoveryAuthority: { status: "identity_conflict", incarnation: "inc-mismatch", daemonEpoch: "epoch-mismatch" } });
    // Keep observer through explicitCompleted await (bounded by test timeout):
    const explicitSettled = await explicitCompleted.promise;
    expect(explicitSettled).toEqual({ kind: "blocked", reason: "identity_conflict" });
    expect(tauriMocks.spawnTerminal).not.toHaveBeenCalled();

    // Assert stale admission remains false and conflict is retained after full async continuation:
    expect(staleProbeAttemptedAdmission).toBe(false);
    expect(getWorkspaceSnapshot("local")!.sessions["sess-1"].backendUnavailableReason).toBe("identity_conflict");
    expect(getWorkspaceSnapshot("local")!.sessions["sess-1"].remoteConnectionState).not.toBe("reconnecting");

    // Unsubscribe finally:
    unsubscribeCache();
    unsubscribeExplicit();
  });
  it("does not admit restored key and token fields without a successful owner receipt", async () => {
    const snapshot = await load();
    setWorkspaceSnapshot("local", { ...snapshot, sessions: { ...snapshot.sessions, "sess-1": {
      ...snapshot.sessions["sess-1"], lifecycle: "running", remoteConnectionState: undefined,
      reconnectLifecycle: "idle", recoveryKey: "restored-key", recoveryToken: 73,
    } } });
    const held = deferred<ReturnType<typeof running>>();
    tauriMocks.describeTerminal.mockReturnValue(held.promise);
    setLocalRecoveryPersistence(async () => {});
    const results: LocalRecoveryAdmissionResult[] = [];
    const unsubscribe = requestLocalRecoveryAdmission("local", "sess-1", false, result => results.push(result));
    expect(results).toEqual([{ kind: "pending" }]);
    unsubscribe(); held.resolve(running());
  });
  it("classifies a missing retained binding as cancelled rather than affirmative exit", async () => {
    const snapshot = await load();
    setWorkspaceSnapshot("local", { ...snapshot, sessions: { ...snapshot.sessions, "sess-1": {
      ...snapshot.sessions["sess-1"], backendSessionId: null, lifecycle: "running",
    } } });
    const results: LocalRecoveryAdmissionResult[] = [];
    requestLocalRecoveryAdmission("local", "sess-1", true, result => results.push(result));
    expect(results).toEqual([{ kind: "cancelled" }]);
    expect(tauriMocks.describeTerminal).not.toHaveBeenCalled();
  });
  it("revalidates an admitted identity and waits for its next save before ready", async () => {
    await load();
    tauriMocks.describeTerminal.mockResolvedValue(running());
    const firstReady = deferred<void>(); const secondReady = deferred<void>();
    const savingAgain = deferred<void>(); const secondSave = deferred<void>();
    const persist = vi.fn().mockResolvedValueOnce(undefined).mockImplementation(() => {
      savingAgain.resolve(); return secondSave.promise;
    });
    setLocalRecoveryPersistence(persist);
    requestLocalRecoveryAdmission("local", "sess-1", true, result => {
      if (result.kind === "ready") firstReady.resolve();
    });
    await act(async () => { await firstReady.promise; });
    const admitted = getWorkspaceSnapshot("local")!.sessions["sess-1"];
    const synchronous: LocalRecoveryAdmissionResult[] = [];
    requestLocalRecoveryAdmission("local", "sess-1", false, result => synchronous.push(result));
    expect(synchronous).toEqual([{ kind: "ready", session: admitted }]);
    const results: LocalRecoveryAdmissionResult[] = [];
    requestLocalRecoveryAdmission("local", "sess-1", true, result => {
      results.push(result); if (result.kind === "ready") secondReady.resolve();
    });
    await act(async () => { await savingAgain.promise; });
    expect(results).toEqual([{ kind: "pending" }]);
    await act(async () => { secondSave.resolve(); await secondReady.promise; });
    expect(persist).toHaveBeenCalledTimes(2);
    expect(results[1]).toEqual({ kind: "ready", session: getWorkspaceSnapshot("local")!.sessions["sess-1"] });
    expect(getWorkspaceSnapshot("local")!.sessions["sess-1"].backendSessionId).toBe(admitted.backendSessionId);
  });
  it("coalesces existing in-flight admission wakes into one immediate retry", async () => {
    await load();
    const first = deferred<ReturnType<typeof running> | null>();
    const second = deferred<ReturnType<typeof running>>(); const retryStarted = deferred<void>();
    tauriMocks.describeTerminal.mockReturnValueOnce(first.promise).mockImplementation(() => {
      retryStarted.resolve(); return second.promise;
    });
    setLocalRecoveryPersistence(async () => {});
    const results = [[], [], []] as LocalRecoveryAdmissionResult[][];
    const ready = deferred<void>(); let readyCount = 0;
    const unsubscribes = results.map(list => requestLocalRecoveryAdmission("local", "sess-1", true, result => {
      list.push(result); if (result.kind === "ready" && ++readyCount === 3) ready.resolve();
    }));
    expect(tauriMocks.describeTerminal).toHaveBeenCalledTimes(1);
    await act(async () => { first.resolve(null); await vi.advanceTimersByTimeAsync(0); await retryStarted.promise; });
    expect(tauriMocks.describeTerminal).toHaveBeenCalledTimes(2);
    expect(results).toEqual([[{ kind: "pending" }], [{ kind: "pending" }], [{ kind: "pending" }]]);
    await act(async () => { second.resolve(running()); await ready.promise; });
    expect(tauriMocks.describeTerminal).toHaveBeenCalledTimes(2);
    for (const list of results) expect(list.map(result => result.kind)).toEqual(["pending", "ready"]);
    unsubscribes.forEach(unsubscribe => unsubscribe());
  });
  it.each(["key", "token", "backend", "incarnation", "epoch", "state", "remove"])(
    "invalidates the admission receipt on %s replacement even if the old tuple returns", async change => {
      await load(); tauriMocks.describeTerminal.mockResolvedValue(running());
      setLocalRecoveryPersistence(async () => {});
      const ready = deferred<void>();
      requestLocalRecoveryAdmission("local", "sess-1", true, result => { if (result.kind === "ready") ready.resolve(); });
      await act(async () => { await ready.promise; });
      const snapshot = getWorkspaceSnapshot("local")!;
      const admitted = snapshot.sessions["sess-1"];
      const changed = { ...admitted };
      if (change === "key") changed.recoveryKey = "replacement";
      if (change === "token") changed.recoveryToken = (admitted.recoveryToken ?? 0) + 1;
      if (change === "backend") changed.backendSessionId = "replacement";
      if (change === "incarnation") changed.incarnation = "replacement";
      if (change === "epoch") changed.daemonEpoch = "replacement";
      if (change === "state") changed.lifecycle = "failed";
      setWorkspaceSnapshot("local", { ...snapshot, sessions: change === "remove" ? {} : { ...snapshot.sessions, "sess-1": changed } });
      setWorkspaceSnapshot("local", snapshot);
      const held = deferred<ReturnType<typeof running>>();
      tauriMocks.describeTerminal.mockReturnValue(held.promise);
      const results: LocalRecoveryAdmissionResult[] = [];
      const unsubscribe = requestLocalRecoveryAdmission("local", "sess-1", false, result => results.push(result));
      expect(results).toEqual([{ kind: "pending" }]);
      unsubscribe(); held.resolve(running());
    },
  );
  it("recovers a loaded background identity after more than 30 seconds and persists before ready", async () => {
    await load();
    let available = false;
    tauriMocks.describeTerminal.mockImplementation(async () => available ? running() : null);
    const persisted = deferred<void>();
    const persisting = deferred<void>();
    const ready = deferred<void>();
    const unsubscribe = subscribeWorkspaceSnapshots(() => {
      if (getWorkspaceSnapshot("local")?.sessions["sess-1"].daemonEpoch === "new") ready.resolve();
    });
    setLocalRecoveryPersistence(async (_workspaceId, candidate, current) => {
      expect(current()).toBe(true);
      expect(candidate.sessions["sess-1"]).toMatchObject({ daemonEpoch: "new", incarnation: "process", lifecycle: "running" });
      expect(getWorkspaceSnapshot("local")?.sessions["sess-1"].remoteConnectionState).toBe("reconnecting");
      persisting.resolve();
      await persisted.promise;
    });
    await vi.advanceTimersByTimeAsync(40_000);
    expect(tauriMocks.describeTerminal.mock.calls.length).toBeGreaterThan(12);
    expect(getWorkspaceSnapshot("local")?.sessions["sess-1"].backendSessionId).toBe("backend");
    available = true;
    await vi.advanceTimersByTimeAsync(4_000);
    await persisting.promise;
    expect(getWorkspaceSnapshot("local")?.sessions["sess-1"].daemonEpoch).toBe("old");
    persisted.resolve();
    await ready.promise;
    expect(getWorkspaceSnapshot("local")?.sessions["sess-1"]).toMatchObject({ backendSessionId: "backend", incarnation: "process", daemonEpoch: "new", lifecycle: "running" });
    unsubscribe();
  });
  it("keeps a loaded recovery owner across hook switch and unmount without closing its PTY", async () => {
    const snapshot = await load();
    const reply = deferred<ReturnType<typeof running>>();
    const entered = deferred<void>();
    const ready = deferred<void>();
    const closeTerminal = vi.fn(async () => {});
    const service: WorkspaceServices = { ensureTerminalEvents: async () => {}, spawnTerminal: async () => "unused",
      getTerminalCwd: async () => null, closeTerminal, waitForTerminalExit: async () => {} };
    tauriMocks.describeTerminal.mockImplementation(() => { entered.resolve(); return reply.promise; });
    const unsubscribe = subscribeWorkspaceSnapshots(() => {
      if (getWorkspaceSnapshot("local")?.sessions["sess-1"].daemonEpoch === "new") ready.resolve();
    });
    const hook = renderHook(({ workspaceId }) => useWorkspaceStore({ workspaceId, services: service }),
      { initialProps: { workspaceId: "local" } });
    act(() => hook.result.current.restoreWorkspace(snapshot));
    setLocalRecoveryPersistence(async () => {});
    await entered.promise;
    hook.rerender({ workspaceId: "other" });
    hook.unmount();
    reply.resolve(running());
    await ready.promise;
    expect(getWorkspaceSnapshot("local")?.sessions["sess-1"]).toMatchObject({ backendSessionId: "backend", incarnation: "process", lifecycle: "running" });
    expect(closeTerminal).not.toHaveBeenCalled();
    expect(tauriMocks.describeTerminal).toHaveBeenCalledTimes(1);
    unsubscribe();
  });
  it("recovers an active running session automatically after an ordinary backend-unavailable event", async () => {
    const snapshot = await load();
    let available = true;
    tauriMocks.describeTerminal.mockImplementation(async () => available ? running() : null);
    const spawnTerminal = vi.fn(async () => "unexpected-spawn");
    const closeTerminal = vi.fn(async () => {});
    const service: WorkspaceServices = { ensureTerminalEvents: async () => {}, spawnTerminal,
      getTerminalCwd: async () => null, closeTerminal, waitForTerminalExit: async () => {} };
    const live = () => getWorkspaceSnapshot("local")?.sessions["sess-1"];
    const isLive = () => live()?.lifecycle === "running" && live()?.daemonEpoch === "new" &&
      live()?.remoteConnectionState === undefined;
    let firstReady = deferred<void>();
    const unsubscribe = subscribeWorkspaceSnapshots(() => { if (isLive()) firstReady.resolve(); });
    const hook = renderHook(() => useWorkspaceStore({ workspaceId: "local", services: service }));
    act(() => hook.result.current.restoreWorkspace(snapshot));
    setLocalRecoveryPersistence(async () => {});
    await act(async () => { await vi.advanceTimersByTimeAsync(4_000); await firstReady.promise; });
    const probesBeforeLoss = tauriMocks.describeTerminal.mock.calls.length;

    // An ordinary loss on a live, non-sleeping pane: the owner is briefly unreachable.
    available = false;
    firstReady = deferred<void>();
    act(() => hook.result.current.markBackendSessionUnavailable("sess-1", "backend", "stream-ended"));
    expect(live()).toMatchObject({ backendSessionId: "backend", incarnation: "process", remoteConnectionState: "reconnecting" });
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    expect(tauriMocks.describeTerminal.mock.calls.length).toBeGreaterThan(probesBeforeLoss + 2);
    expect(live()?.remoteConnectionState).toBe("reconnecting");

    available = true;
    await act(async () => { await vi.advanceTimersByTimeAsync(4_000); await firstReady.promise; });
    expect(live()).toMatchObject({ backendSessionId: "backend", incarnation: "process", daemonEpoch: "new", lifecycle: "running" });
    expect(spawnTerminal).not.toHaveBeenCalled();
    expect(closeTerminal).not.toHaveBeenCalled();
    unsubscribe();
    hook.unmount();
  });
  it("coalesces manual Retry during an in-flight probe", async () => {
    await load();
    const reply = deferred<ReturnType<typeof running>>();
    const entered = deferred<void>();
    tauriMocks.describeTerminal.mockImplementation(() => { entered.resolve(); return reply.promise; });
    setLocalRecoveryPersistence(async () => {});
    await entered.promise;
    wakeLocalSessionRecovery("local", "sess-1"); wakeLocalSessionRecovery("local", "sess-1");
    expect(tauriMocks.describeTerminal).toHaveBeenCalledTimes(1);
    clearWorkspaceSnapshot("local"); reply.resolve(running());
  });
  it.each(["remove", "close", "rebind", "token"])("rejects a late reply after %s without persistence", async (change) => {
    const snapshot = await load();
    const reply = deferred<ReturnType<typeof running>>();
    const replacementReply = deferred<ReturnType<typeof running>>();
    const entered = deferred<void>();
    tauriMocks.describeTerminal.mockImplementationOnce(() => { entered.resolve(); return reply.promise; })
      .mockImplementation(() => replacementReply.promise);
    const persist = vi.fn(async () => {});
    setLocalRecoveryPersistence(persist); await entered.promise;
    if (change === "remove") clearWorkspaceSnapshot("local");
    else {
      const current = getWorkspaceSnapshot("local")!;
      const session = current.sessions["sess-1"];
      setWorkspaceSnapshot("local", { ...current, sessions: change === "close" ? {} : {
        ...current.sessions, "sess-1": { ...session,
          ...(change === "rebind" ? { backendSessionId: "replacement" } : { recoveryToken: (session.recoveryToken ?? 0) + 1 }),
        },
      } });
    }
    reply.resolve(running());
    await reply.promise;
    expect(persist).not.toHaveBeenCalled();
    expect(getWorkspaceSnapshot("local")?.sessions["sess-1"]?.daemonEpoch).not.toBe("new");
    expect(snapshot.sessions["sess-1"].backendSessionId).toBe("backend");
  });
  it.each(["token", "key"])("automatically recovers an external %s replacement and rejects the old affirmative exit", async (change) => {
    await load();
    const oldReply = deferred<Awaited<ReturnType<typeof import("../lib/tauri").describeTerminal>>>();
    const replacementReply = deferred<ReturnType<typeof running>>();
    const entered = deferred<void>();
    const replacementEntered = deferred<void>();
    const ready = deferred<void>();
    tauriMocks.describeTerminal.mockImplementationOnce(() => { entered.resolve(); return oldReply.promise; })
      .mockImplementationOnce(() => { replacementEntered.resolve(); return replacementReply.promise; });
    const persist = vi.fn(async () => {});
    const unsubscribe = subscribeWorkspaceSnapshots(() => {
      if (getWorkspaceSnapshot("local")?.sessions["sess-1"].daemonEpoch === "new") ready.resolve();
    });
    setLocalRecoveryPersistence(persist);
    await entered.promise;
    const current = getWorkspaceSnapshot("local")!;
    const session = current.sessions["sess-1"];
    setWorkspaceSnapshot("local", { ...current, sessions: { ...current.sessions, "sess-1": {
      ...session,
      ...(change === "token" ? { recoveryToken: (session.recoveryToken ?? 0) + 1 } : { recoveryKey: "external-owner" }),
    } } });
    await replacementEntered.promise;
    expect(tauriMocks.describeTerminal).toHaveBeenCalledTimes(2);
    replacementReply.resolve(running());
    await ready.promise;
    const admitted = getWorkspaceSnapshot("local");
    oldReply.resolve({ ...running(), cols: 80, rows: 24, running: false, daemonEpoch: "old",
      recoveryAuthority: { status: "affirmative_exit", incarnation: "process", daemonEpoch: "old", exitCode: 0 } });
    await oldReply.promise;
    expect(getWorkspaceSnapshot("local")).toBe(admitted);
    expect(admitted?.sessions["sess-1"]).toMatchObject({ backendSessionId: "backend", incarnation: "process", daemonEpoch: "new", lifecycle: "running" });
    expect(persist).toHaveBeenCalledTimes(1);
    unsubscribe();
  });
  it("projects identity conflict as blocked without spawning or clearing retained identity", async () => {
    await load();
    const blocked = deferred<void>();
    const unsubscribe = subscribeWorkspaceSnapshots(() => {
      if (getWorkspaceSnapshot("local")?.sessions["sess-1"].reconnectLifecycle === "failed") blocked.resolve();
    });
    tauriMocks.describeTerminal.mockResolvedValue({ ...running(), incarnation: "different" });
    const persist = vi.fn(async () => {});
    setLocalRecoveryPersistence(persist); await blocked.promise;
    expect(persist).not.toHaveBeenCalled();
    expect(getWorkspaceSnapshot("local")?.sessions["sess-1"].backendSessionId).toBe("backend");
    await vi.advanceTimersByTimeAsync(40_000);
    expect(tauriMocks.describeTerminal).toHaveBeenCalledTimes(1);
    unsubscribe();
  });
  it("notifies only real cache changes, synchronously after mutation", async () => {
    const snapshot = await load();
    const listener = vi.fn(() => { expect(getWorkspaceSnapshot("other")).toBe(snapshot); });
    const unsubscribe = subscribeWorkspaceSnapshots(listener);
    setWorkspaceSnapshot("other", snapshot); setWorkspaceSnapshot("other", snapshot);
    clearWorkspaceSnapshot("absent");
    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe(); clearWorkspaceSnapshot("other");
    expect(listener).toHaveBeenCalledTimes(1);
  });
  it("retains unknown identity after rejected persistence and retries through the same owner", async () => {
    await load();
    tauriMocks.describeTerminal.mockResolvedValue(running());
    const rejected = deferred<void>();
    const ready = deferred<void>();
    const unsubscribe = subscribeWorkspaceSnapshots(() => {
      if (getWorkspaceSnapshot("local")?.sessions["sess-1"].daemonEpoch === "new") ready.resolve();
    });
    let writes = 0;
    setLocalRecoveryPersistence(async () => {
      if (++writes === 1) { rejected.resolve(); throw new Error("disk unavailable"); }
    });
    await rejected.promise;
    expect(getWorkspaceSnapshot("local")?.sessions["sess-1"]).toMatchObject({ backendSessionId: "backend", daemonEpoch: "old", remoteConnectionState: "reconnecting" });
    await vi.advanceTimersByTimeAsync(250);
    await ready.promise;
    expect(writes).toBe(2);
    unsubscribe();
  });
  it("rejects old admission when token changes during pending persistence", async () => {
    await load();
    tauriMocks.describeTerminal.mockResolvedValue(running());
    const entered = deferred<void>();
    const release = deferred<void>();
    const resumed = deferred<void>();
    let oldCurrent!: () => boolean;
    const persist = vi.fn(async (_id: string, _state: WorkspaceState, current: () => boolean) => {
      oldCurrent = current; entered.resolve(); await release.promise; resumed.resolve();
    });
    setLocalRecoveryPersistence(persist);
    await entered.promise;
    const snapshot = getWorkspaceSnapshot("local")!;
    tauriMocks.describeTerminal.mockImplementation(() => new Promise(() => {}));
    setWorkspaceSnapshot("local", { ...snapshot, sessions: { ...snapshot.sessions, "sess-1": {
      ...snapshot.sessions["sess-1"], recoveryToken: (snapshot.sessions["sess-1"].recoveryToken ?? 0) + 1,
    } } });
    expect(oldCurrent()).toBe(false);
    release.resolve(); await resumed.promise; await persist.mock.results[0].value;
    expect(getWorkspaceSnapshot("local")?.sessions["sess-1"].daemonEpoch).toBe("old");
    expect(persist).toHaveBeenCalledTimes(1);
  });
  it("admits a session with a completed local split spawnIntent (ready: true) instead of cancelling", async () => {
    const snapshot = await load();
    setWorkspaceSnapshot("local", {
      ...snapshot,
      sessions: {
        ...snapshot.sessions,
        "sess-1": {
          ...snapshot.sessions["sess-1"],
          spawnIntent: { requestId: "split-req-1", generation: 1, ready: true },
        },
      },
    });
    tauriMocks.describeTerminal.mockResolvedValue(running());
    const persist = vi.fn(async () => {});
    setLocalRecoveryPersistence(persist);
    const results: LocalRecoveryAdmissionResult[] = [];
    const ready = deferred<void>();
    const unsubscribe = requestLocalRecoveryAdmission("local", "sess-1", true, (result) => {
      results.push(result);
      if (result.kind === "ready") ready.resolve();
    });
    await ready.promise;
    expect(results.map((r) => r.kind)).toContain("ready");
    expect(results.map((r) => r.kind)).not.toContain("cancelled");
    unsubscribe();
  });
  it("admits recovery when DaemonSessionDetails omits top-level daemonEpoch and provides recoveryAuthority.daemonEpoch", async () => {
    await load();
    const { daemonEpoch: _omitted, ...wireDetails } = running();
    tauriMocks.describeTerminal.mockResolvedValue(wireDetails as ReturnType<typeof running>);
    const persist = vi.fn(async () => {});
    setLocalRecoveryPersistence(persist);
    const results: LocalRecoveryAdmissionResult[] = [];
    const ready = deferred<void>();
    const unsubscribe = requestLocalRecoveryAdmission("local", "sess-1", true, (result) => {
      results.push(result);
      if (result.kind === "ready") ready.resolve();
    });
    await ready.promise;
    expect(results.map((r) => r.kind)).toContain("ready");
    expect(getWorkspaceSnapshot("local")?.sessions["sess-1"].daemonEpoch).toBe("new");
    unsubscribe();
  });
});
