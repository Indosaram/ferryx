import { useCallback } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useWorkspaceStore } from "./workspaceStore";
import type { WorkspaceState } from "./workspaceStore";
import { resetWorkspaceRestore, useWorkspaceRestore } from "./workspaceRestore";
import { useWorkspaceRuntime } from "./workspaceRuntime";
import { clearHmrWorkspaceState } from "./hmrWorkspaceState";
import { clearWorkspaceSnapshot } from "./workspaceSnapshotCache";
import type { Worktree } from "../lib/types";

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc",
  branch: "refs/heads/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

describe("workspace restore duplicate tab guard", () => {
  beforeEach(() => {
    resetWorkspaceRestore();
    clearHmrWorkspaceState();
    clearWorkspaceSnapshot();
  });
  afterEach(() => {
    cleanup();
    resetWorkspaceRestore();
    clearHmrWorkspaceState();
    clearWorkspaceSnapshot();
  });

  it("does not create an extra empty terminal tab when async restore is in flight", async () => {
    let sessionCounter = 0;
    const services = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      listWorktrees: vi.fn(async () => [worktree]),
      onWorktreeChanged: vi.fn(async () => () => undefined),
      isTauriRuntime: vi.fn(() => true),
      spawnTerminal: vi.fn(async () => `backend-${++sessionCounter}`),
      closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
      getTerminalCwd: vi.fn(async () => worktree.path),
    };

    const savedSession = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: "/repo/main",
          worktrees: [worktree],
          activeWorktreePath: "/repo/main",
          layout: {
            splitMode: "none",
            primaryTabId: "tab-restored-1",
            activeTabId: "tab-restored-1",
            tabs: [
              {
                id: "tab-restored-1",
                kind: "terminal",
                label: "main",
                terminal: {
                  primarySessionId: "session-restored-1",
                  paneTree: { leafId: "leaf-1", type: "leaf" },
                  sessionIdsByLeafId: { "leaf-1": "session-restored-1" },
                },
              },
            ],
          },
          terminalSessions: {
            "session-restored-1": {
              localSessionId: "session-restored-1",
              backendSessionId: "backend-existing",
              worktreePath: "/repo/main",
              cwd: "/repo/main",
              daemonEpoch: null,
              lastOutputSequence: null,
            },
          },
        },
      },
    };

    let resolveLoad = (_value: typeof savedSession) => {};
    const load = new Promise<typeof savedSession>((resolve) => { resolveLoad = resolve; });
    let resolveRestored = () => {};
    const restored = new Promise<void>((resolve) => { resolveRestored = resolve; });
    const loadSessionFn = vi.fn(() => load);
    const listLiveBackendSessionIdsFn = vi.fn(async () =>
      [{ sessionId: "backend-existing", daemonEpoch: null, worktreePath: "/repo/main" }],
    );

    const { result, rerender } = renderHook<
      { store: ReturnType<typeof useWorkspaceStore>; runtime: ReturnType<typeof useWorkspaceRuntime> },
      { registeredProjectId: string | null }
    >(
      ({ registeredProjectId }: { registeredProjectId: string | null }) => {
        const store = useWorkspaceStore({
          workspaceId: "default",
          services,
        });

        const runtime = useWorkspaceRuntime({
          workspaceId: "default",
          activeWorktreePath: store.state.activeWorktreePath,
          syncWorktrees: store.syncWorktrees,
          ensureTabForWorktree: store.ensureTabForWorktree,
          registeredWorkspaceId: registeredProjectId,
          services,
        });

        const restoreWorkspace = useCallback((state: WorkspaceState) => {
          store.restoreWorkspace(state);
          resolveRestored();
        }, [store.restoreWorkspace]);
        useWorkspaceRestore({
          workspaceId: "default",
          recoveredFromHmr: store.recoveredFromHmr,
          restoreWorkspace,
          loadSessionFn,
          listLiveBackendSessionIdsFn,
          enabled: registeredProjectId === "default",
        });

        return { store, runtime };
      },
      { initialProps: { registeredProjectId: null } },
    );

    expect(result.current.store.state.layout.tabs).toHaveLength(0);

    await act(async () => {
      await result.current.runtime.refreshWorktrees({ allowCreate: false });
    });

    expect(result.current.store.state.layout.tabs).toHaveLength(0);

    rerender({ registeredProjectId: "default" });

    expect(loadSessionFn).toHaveBeenCalled();
    await act(async () => { await result.current.runtime.refreshWorktrees(); });
    expect(services.spawnTerminal).not.toHaveBeenCalled();

    await act(async () => {
      resolveLoad(savedSession);
      await restored;
    });

    const tabs = result.current.store.state.layout.tabs;
    expect(tabs).toHaveLength(1);
    expect(tabs[0].id).toBe("tab-restored-1");
    expect(services.spawnTerminal).not.toHaveBeenCalled();
  });
});
