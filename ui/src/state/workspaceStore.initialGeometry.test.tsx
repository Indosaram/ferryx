import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";
import { resetWorkspaceRestore } from "./workspaceRestore";
import { clearHmrWorkspaceState } from "./hmrWorkspaceState";
import { clearWorkspaceSnapshot } from "./workspaceSnapshotCache";
import type { InitialGeometryTarget, InitialPtyGrid } from "../lib/initialPtyGeometry";
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

function servicesWith(measure: (target: InitialGeometryTarget) => Promise<InitialPtyGrid | null>) {
  let next = 0;
  const spawnTerminal = vi.fn(async (_request: Parameters<WorkspaceServices["spawnTerminal"]>[0]) => `backend-${++next}`);
  const splitOperation = vi.fn<NonNullable<WorkspaceServices["splitOperation"]>>(async (_request) => {
    throw { code: "INTERNAL_ERROR", message: "stop after prepare request" };
  });
  const services: WorkspaceServices = {
    ensureTerminalEvents: vi.fn(async () => undefined),
    spawnTerminal,
    getTerminalCwd: vi.fn(async () => worktree.path),
    closeTerminal: vi.fn(async () => undefined),
    waitForTerminalExit: vi.fn(async () => undefined),
    splitOperation,
    measureInitialGeometry: vi.fn(measure),
  };
  return { services, spawnTerminal, splitOperation };
}

describe("initial PTY geometry callers", () => {
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

  it("new tab spawns with the measured group-body grid", async () => {
    const { services, spawnTerminal } = servicesWith(async () => ({ cols: 132, rows: 41 }));
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId: "ws-geo", initialWorktrees: [worktree], services }));
    await act(async () => { await result.current.openTab(worktree, "existing", "existing-backend"); });
    await act(async () => { await result.current.openTab(worktree); });
    expect(services.measureInitialGeometry).toHaveBeenCalledWith({ kind: "newTab", groupId: expect.anything() });
    expect(spawnTerminal).toHaveBeenCalledTimes(1);
    expect(spawnTerminal.mock.calls[0][0]).toMatchObject({ cols: 132, rows: 41 });
  });

  it("new tab omits cols/rows when geometry is unavailable", async () => {
    const { services, spawnTerminal } = servicesWith(async () => null);
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId: "ws-geo", initialWorktrees: [worktree], services }));
    await act(async () => { await result.current.openTab(worktree); });
    const request = spawnTerminal.mock.calls[0][0];
    expect("cols" in request).toBe(false);
    expect("rows" in request).toBe(false);
  });

  it("a failing measurement never blocks the spawn", async () => {
    const { services, spawnTerminal } = servicesWith(async () => { throw new Error("no window"); });
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId: "ws-geo", initialWorktrees: [worktree], services }));
    await act(async () => { await result.current.openTab(worktree); });
    expect(spawnTerminal).toHaveBeenCalledTimes(1);
    expect("cols" in spawnTerminal.mock.calls[0][0]).toBe(false);
  });

  it.each([false, true])("recovery measures each session before spawning (batch=%s)", async (batch) => {
    const { services, spawnTerminal } = servicesWith(async (target) => {
      if (target.kind !== "session") throw new Error("recovery must measure a session viewport");
      return target.sessionId === "visible" ? { cols: 132, rows: 40 } : null;
    });
    const spawnTerminalsBatch = vi.fn<NonNullable<WorkspaceServices["spawnTerminalsBatch"]>>(
      async (requests) => requests.map((_, index) => ({ index, sessionId: `batch-${index}`, error: null })),
    );
    if (batch) services.spawnTerminalsBatch = spawnTerminalsBatch;
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId: "ws-geo", services }));
    act(() => {
      result.current.restoreWorkspace({
        workspaceId: "ws-geo",
        worktrees: [worktree],
        activeWorktreePath: worktree.path,
        layout: { tabs: [], activeTabId: null, layoutsByTabId: {} },
        sessions: {
          visible: { id: "visible", backendSessionId: null, lifecycle: "exited", workspaceId: "ws-geo", worktree: null, cwd: worktree.path },
          hidden: { id: "hidden", backendSessionId: null, lifecycle: "exited", workspaceId: "ws-geo", worktree: null, cwd: worktree.path },
        },
        unreadTabIds: {},
        unreadWorktreePaths: {},
      });
    });

    await act(async () => { await result.current.ensureSessionBackends(["visible", "hidden"]); });

    expect(services.measureInitialGeometry).toHaveBeenCalledWith({ kind: "session", sessionId: "visible" });
    expect(services.measureInitialGeometry).toHaveBeenCalledWith({ kind: "session", sessionId: "hidden" });
    const requests = batch ? spawnTerminalsBatch.mock.calls[0][0] : spawnTerminal.mock.calls.map(([request]) => request);
    expect(requests).toHaveLength(2);
    expect(requests[0]).toMatchObject({ cols: 132, rows: 40 });
    expect(requests[1]).not.toHaveProperty("cols");
    expect(requests[1]).not.toHaveProperty("rows");
    expect(result.current.state.sessions.visible.backendSessionId).toBeTruthy();
    expect(result.current.state.sessions.hidden.backendSessionId).toBeTruthy();
  });

  it("local split measures the committed new pane and sends the grid in the prepare request", async () => {
    let sessionCommittedAtMeasure = false;
    let stateProbe: (() => Record<string, unknown>) | null = null;
    const { services, splitOperation } = servicesWith(async (target) => {
      if (target.kind === "session") sessionCommittedAtMeasure = Boolean(stateProbe?.()[target.sessionId]);
      return target.kind === "session" ? { cols: 61, rows: 37 } : { cols: 120, rows: 40 };
    });
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId: "ws-geo", initialWorktrees: [worktree], services }));
    stateProbe = () => result.current.state.sessions;
    await act(async () => { await result.current.openTab(worktree); });
    const tab = result.current.state.layout.tabs[0];
    const tabLayout = result.current.state.layout.layoutsByTabId[tab.id];
    await act(async () => {
      await result.current.splitPane(tab.id, tabLayout.activeLeafId!, "horizontal");
    });
    expect(sessionCommittedAtMeasure).toBe(true);
    expect(splitOperation).toHaveBeenCalledTimes(1);
    expect(splitOperation.mock.calls[0][0]).toMatchObject({
      action: "prepare",
      request: { cols: 61, rows: 37 },
    });
  });
});
