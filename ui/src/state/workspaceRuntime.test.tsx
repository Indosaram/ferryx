import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const switchDebugMock = vi.hoisted(() => vi.fn());
vi.mock("../lib/switchDebug", () => ({
  switchDebug: switchDebugMock,
}));

import type { Worktree, WorktreeChangedPayload } from "../lib/types";
import { useWorkspaceRuntime, type WorkspaceRuntimeServices } from "./workspaceRuntime";
import { useWorkspaceStore } from "./workspaceStore";

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function createServices() {
  let sessionNumber = 0;
  let worktreeChangedHandler: ((payload: WorktreeChangedPayload) => void) | null = null;
  const services = {
    ensureTerminalEvents: vi.fn(async () => undefined),
    listWorktrees: vi.fn(async () => [worktree]),
    onWorktreeChanged: vi.fn(async (handler) => {
      worktreeChangedHandler = handler;
      return () => undefined;
    }),
    isTauriRuntime: vi.fn(() => true),
    spawnTerminal: vi.fn(async () => `backend-${++sessionNumber}`),
    closeTerminal: vi.fn(async () => undefined),
    waitForTerminalExit: vi.fn(async () => undefined),
    getTerminalCwd: vi.fn(async () => worktree.path),
  };
  return {
    services,
    emitWorktreeChanged: (payload: WorktreeChangedPayload) => worktreeChangedHandler?.(payload),
  };
}

describe("useWorkspaceRuntime", () => {
  it("registers terminal/worktree listeners before the initial refresh and opens a valid tab", async () => {
    const { services } = createServices();
    const syncWorktrees = vi.fn(async () => undefined);
    const ensureTabForWorktree = vi.fn(async () => "tab-main");
    const { result } = renderHook(() =>
      useWorkspaceRuntime({
        activeWorktreePath: null,
        syncWorktrees,
        ensureTabForWorktree,
        services,
      }),
    );

    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(1));

    expect(services.ensureTerminalEvents).toHaveBeenCalledBefore(services.listWorktrees as ReturnType<typeof vi.fn>);
    expect(services.onWorktreeChanged).toHaveBeenCalledBefore(services.listWorktrees as ReturnType<typeof vi.fn>);
    expect(syncWorktrees).toHaveBeenCalledWith([worktree]);
    expect(ensureTabForWorktree).toHaveBeenCalledWith(worktree);
    expect(result.current.runtimeError).toBeNull();
  });

  it("refreshes worktrees when the window regains focus", async () => {
    const { services } = createServices();
    const { result } = renderHook(() =>
      useWorkspaceRuntime({
        activeWorktreePath: worktree.path,
        syncWorktrees: vi.fn(async () => undefined),
        ensureTabForWorktree: vi.fn(async () => "tab-main"),
        services,
      }),
    );

    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(1));
    act(() => window.dispatchEvent(new Event("focus")));
    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(2));

    expect(result.current.runtimeError).toBeNull();
  });

  it("refreshes worktrees when the backend emits worktree_changed", async () => {
    const { services, emitWorktreeChanged } = createServices();
    renderHook(() =>
      useWorkspaceRuntime({
        activeWorktreePath: worktree.path,
        syncWorktrees: vi.fn(async () => undefined),
        ensureTabForWorktree: vi.fn(async () => "tab-main"),
        services,
      }),
    );

    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(1));
    act(() =>
      emitWorktreeChanged({
        workspaceId: "default",
        worktree: { wsId: "ws-main", slug: "main" },
        kind: "created",
      }),
    );
    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(2));
  });

  it("never syncs a stale in-flight refresh into the workspace that replaced it", async () => {
    const deferrals: Array<{ workspaceId: string; resolve: (value: Worktree[]) => void }> = [];
    const services: WorkspaceRuntimeServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      listWorktrees: vi.fn(
        (workspaceId: string) =>
          new Promise<Worktree[]>((resolve) => {
            deferrals.push({ workspaceId, resolve });
          }),
      ),
      onWorktreeChanged: vi.fn(async () => () => undefined),
      isTauriRuntime: vi.fn(() => true),
    };
    const syncedWorkspaces: Array<{ workspaceId: string; paths: string[] }> = [];
    const syncWorktrees = vi.fn(async (list: Worktree[]) => {
      syncedWorkspaces.push({
        workspaceId: currentWorkspaceId,
        paths: list.map((candidate) => candidate.path),
      });
    });
    let currentWorkspaceId = "project-a";

    const { rerender } = renderHook(
      ({ workspaceId }: { workspaceId: string }) => {
        currentWorkspaceId = workspaceId;
        return useWorkspaceRuntime({
          workspaceId,
          activeWorktreePath: null,
          syncWorktrees,
          ensureTabForWorktree: vi.fn(async () => "tab-1"),
          services,
        });
      },
      { initialProps: { workspaceId: "project-a" } },
    );

    await waitFor(() => expect(deferrals).toHaveLength(1));
    expect(deferrals[0].workspaceId).toBe("project-a");

    rerender({ workspaceId: "project-b" });
    await waitFor(() => expect(deferrals).toHaveLength(2));

    await act(async () => {
      deferrals[0].resolve([{ ...worktree, path: "/repo/project-a-only" }]);
      deferrals[1].resolve([{ ...worktree, path: "/repo/project-b-only" }]);
    });

    await waitFor(() => expect(syncWorktrees).toHaveBeenCalled());
    expect(syncedWorkspaces.some((entry) => entry.paths.includes("/repo/project-a-only"))).toBe(false);
  });

  it("waits for backend registration before listing worktrees, then syncs once registered", async () => {
    const { services } = createServices();
    const syncWorktrees = vi.fn(async () => undefined);
    const ensureTabForWorktree = vi.fn(async () => "tab-main");
    const { rerender } = renderHook(
      ({ registeredWorkspaceId }: { registeredWorkspaceId: string | null }) =>
        useWorkspaceRuntime({
          workspaceId: "superwiki-mail-otp",
          activeWorktreePath: null,
          syncWorktrees,
          ensureTabForWorktree,
          registeredWorkspaceId,
          services,
        }),
      { initialProps: { registeredWorkspaceId: null as string | null } },
    );

    expect(services.ensureTerminalEvents).not.toHaveBeenCalled();
    expect(services.listWorktrees).not.toHaveBeenCalled();

    rerender({ registeredWorkspaceId: "superwiki-mail-otp" });

    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(1));
    expect(services.listWorktrees).toHaveBeenCalledWith("superwiki-mail-otp");
    expect(syncWorktrees).toHaveBeenCalledWith([worktree]);
  });

  it("keeps one listener registration when the plain-root worktree object identity changes", async () => {
    const { services } = createServices();
    services.listWorktrees = vi.fn(async () => []);
    const plainRoot = (): Worktree => ({
      path: "/Users/dev/superwiki-mail-otp",
      head: "",
      branch: null,
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    });
    const syncWorktrees = vi.fn(async () => undefined);
    const { rerender } = renderHook(() =>
      useWorkspaceRuntime({
        workspaceId: "superwiki-mail-otp",
        activeWorktreePath: null,
        syncWorktrees,
        ensureTabForWorktree: vi.fn(async () => "tab-main"),
        plainRootWorktree: plainRoot(),
        services,
      }),
    );

    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(1));
    rerender();
    rerender();
    rerender();

    expect(services.onWorktreeChanged).toHaveBeenCalledTimes(1);
    expect(services.listWorktrees).toHaveBeenCalledTimes(1);
    expect(syncWorktrees).toHaveBeenCalledWith([plainRoot()]);
  });

  it("ignores an older refresh for the same workspace after a newer one resolved", async () => {
    const resolvers: Array<(value: Worktree[]) => void> = [];
    const services: WorkspaceRuntimeServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      listWorktrees: vi.fn(
        () =>
          new Promise<Worktree[]>((resolve) => {
            resolvers.push(resolve);
          }),
      ),
      onWorktreeChanged: vi.fn(async () => () => undefined),
      isTauriRuntime: vi.fn(() => true),
    };
    const syncWorktrees = vi.fn(async (_list: Worktree[]) => undefined);

    const { rerender } = renderHook(
      ({ workspaceId }: { workspaceId: string }) =>
        useWorkspaceRuntime({
          workspaceId,
          activeWorktreePath: null,
          syncWorktrees,
          ensureTabForWorktree: vi.fn(async () => "tab-1"),
          services,
        }),
      { initialProps: { workspaceId: "ws-a" } },
    );

    await waitFor(() => expect(resolvers).toHaveLength(1));

    await act(async () => {
      rerender({ workspaceId: "ws-b" });
    });
    await waitFor(() => expect(resolvers).toHaveLength(2));

    await act(async () => {
      rerender({ workspaceId: "ws-a" });
    });
    await waitFor(() => expect(resolvers).toHaveLength(3));

    const stale = { ...worktree, path: "/repo/stale-a" };
    const fresh = { ...worktree, path: "/repo/fresh-a" };

    await act(async () => {
      resolvers[2]([fresh]);
      await Promise.resolve();
    });
    await act(async () => {
      resolvers[0]([stale]);
      await Promise.resolve();
    });

    const syncedPaths = syncWorktrees.mock.calls.map((call) => call[0].at(0)?.path);
    expect(syncedPaths).toContain("/repo/fresh-a");
    expect(syncedPaths).not.toContain("/repo/stale-a");
  });

  it("extracts error message cleanly instead of logging [object Object]", async () => {
    const { services } = createServices();
    const structuredError = {
      code: "WORKSPACE_NOT_FOUND",
      message: "Workspace ws-err not found on backend",
      details: {},
    };
    services.listWorktrees = vi.fn(async () => {
      throw structuredError;
    });

    const { result, unmount } = renderHook(() =>
      useWorkspaceRuntime({
        workspaceId: "ws-err",
        activeWorktreePath: null,
        syncWorktrees: vi.fn(async () => undefined),
        ensureTabForWorktree: vi.fn(async () => "tab-1"),
        services,
      }),
    );

    await waitFor(() => {
      expect(result.current.runtimeError).toEqual(structuredError);
    });

    expect(switchDebugMock).toHaveBeenCalledWith("workspace.runtime.error", {
      workspaceId: "ws-err",
      error: "Workspace ws-err not found on backend",
      code: "WORKSPACE_NOT_FOUND",
    });

    unmount();
  });

  it("with 4 tabs active and user on tab 4, a refresh cycle does NOT change activeTabId", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => {
      const store = useWorkspaceStore({ initialWorktrees: [worktree], services });
      const runtime = useWorkspaceRuntime({
        workspaceId: "default",
        activeWorktreePath: store.state.activeWorktreePath,
        syncWorktrees: store.syncWorktrees,
        ensureTabForWorktree: store.ensureTabForWorktree,
        services,
      });
      return { store, runtime };
    });

    let tab4Id = "";
    await act(async () => {
      await result.current.store.openTab(worktree, "tab 1");
      await result.current.store.openTab(worktree, "tab 2");
      await result.current.store.openTab(worktree, "tab 3");
      const tab4 = await result.current.store.openTab(worktree, "tab 4");
      tab4Id = tab4 ?? "";
    });

    expect(result.current.store.state.layout.tabs).toHaveLength(4);
    expect(result.current.store.state.layout.activeTabId).toBe(tab4Id);

    await act(async () => {
      await result.current.runtime.refreshWorktrees();
    });

    expect(result.current.store.state.layout.tabs).toHaveLength(4);
    expect(result.current.store.state.layout.activeTabId).toBe(tab4Id);
  });

  it("missing preferred-worktree tab still gets created (existence semantics preserved)", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => {
      const store = useWorkspaceStore({ initialWorktrees: [worktree], services });
      const runtime = useWorkspaceRuntime({
        workspaceId: "default",
        activeWorktreePath: store.state.activeWorktreePath,
        syncWorktrees: store.syncWorktrees,
        ensureTabForWorktree: store.ensureTabForWorktree,
        services,
      });
      return { store, runtime };
    });

    expect(result.current.store.state.layout.tabs).toHaveLength(0);

    await act(async () => {
      await result.current.runtime.refreshWorktrees();
    });

    expect(result.current.store.state.layout.tabs).toHaveLength(1);
    expect(result.current.store.state.layout.tabs[0].label).toBe("main");
  });

  it("an ime-anchor store update does not enqueue a worktree refresh", async () => {
    const { services } = createServices();
    const { rerender } = renderHook(
      (_props: { imeAnchor: { x: number; y: number } }) =>
        useWorkspaceRuntime({
          workspaceId: "default",
          activeWorktreePath: "/repo/main",
          syncWorktrees: vi.fn(async () => undefined),
          ensureTabForWorktree: vi.fn(async () => "tab-1"),
          services,
        }),
      { initialProps: { imeAnchor: { x: 10, y: 20 } } },
    );

    await waitFor(() => expect(services.listWorktrees).toHaveBeenCalledTimes(1));

    act(() => {
      rerender({ imeAnchor: { x: 15, y: 20 } });
      rerender({ imeAnchor: { x: 20, y: 20 } });
      rerender({ imeAnchor: { x: 25, y: 20 } });
    });

    expect(services.listWorktrees).toHaveBeenCalledTimes(1);
  });
});
