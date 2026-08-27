import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { clearWorkspaceSnapshot } from "./workspaceSnapshotCache";
import { useWorkspaceRuntime, type WorkspaceRuntimeServices } from "./workspaceRuntime";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const createBrowserMock = vi.fn();
const closeBrowserMock = vi.fn(async (_browserId: string) => undefined);
vi.mock("../lib/browserTauri", () => ({
  BROWSER_SHORTCUT_EVENT: "ferryx:browser-shortcut",
  createBrowser: (options: unknown) => createBrowserMock(options),
  closeBrowser: (browserId: string) => closeBrowserMock(browserId),
  navigateBrowser: vi.fn(async () => undefined),
  reloadBrowser: vi.fn(async () => undefined),
}));

function gitWorktree(path: string, slug: string, wsId: string): Worktree {
  return {
    path,
    head: "abc123",
    branch: `refs/heads/orca/${wsId}/${slug}`,
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  };
}

const alphaMain = gitWorktree("/repos/alpha", "main", "alpha");
const betaMain = gitWorktree("/repos/beta", "main", "beta");

const storeServices: WorkspaceServices = {
  ensureTerminalEvents: vi.fn(async () => undefined),
  spawnTerminal: vi.fn(async () => "backend-1"),
  getTerminalCwd: vi.fn(async (path: string) => path),
  closeTerminal: vi.fn(async () => undefined),
  waitForTerminalExit: vi.fn(async () => undefined),
};

/**
 * Reproduces the user-reported flow: open project A, switch to project B, then
 * click back to A. The sidebar must show A's worktrees again instead of the
 * "No Git worktrees found for this repository." empty state.
 */
describe("project switch-away-and-back keeps each project's worktrees", () => {
  beforeEach(() => {
    clearWorkspaceSnapshot();
    vi.clearAllMocks();
  });

  it("re-lists worktrees for a project that was already registered before the switch", async () => {
    const worktreesByWorkspace: Record<string, Worktree[]> = {
      alpha: [alphaMain],
      beta: [betaMain],
    };
    const listWorktrees = vi.fn(async (workspaceId: string) => worktreesByWorkspace[workspaceId] ?? []);
    const runtimeServices: WorkspaceRuntimeServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      listWorktrees,
      onWorktreeChanged: vi.fn(async () => () => undefined),
      isTauriRuntime: vi.fn(() => true),
    };

    // Mirrors App.tsx: registeredWorkspaceId is cleared on every project change
    // and set again only after registerProject resolves for the new project.
    function useProjectShell(workspaceId: string, registeredWorkspaceId: string | null) {
      const store = useWorkspaceStore({ workspaceId, services: storeServices });
      useWorkspaceRuntime({
        workspaceId,
        activeWorktreePath: store.state.activeWorktreePath,
        syncWorktrees: store.syncWorktrees,
        ensureTabForWorktree: store.ensureTabForWorktree,
        registeredWorkspaceId,
        services: runtimeServices,
      });
      return store;
    }

    const { result, rerender } = renderHook(
      ({ workspaceId, registeredWorkspaceId }: { workspaceId: string; registeredWorkspaceId: string | null }) =>
        useProjectShell(workspaceId, registeredWorkspaceId),
      { initialProps: { workspaceId: "alpha", registeredWorkspaceId: "alpha" as string | null } },
    );

    await waitFor(() => expect(result.current.state.worktrees).toHaveLength(1));
    expect(result.current.state.worktrees[0].path).toBe("/repos/alpha");

    await act(async () => {
      rerender({ workspaceId: "beta", registeredWorkspaceId: null });
    });
    await act(async () => {
      rerender({ workspaceId: "beta", registeredWorkspaceId: "beta" });
    });

    await waitFor(() => expect(result.current.state.worktrees[0]?.path).toBe("/repos/beta"));

    await act(async () => {
      rerender({ workspaceId: "alpha", registeredWorkspaceId: null });
    });
    await act(async () => {
      rerender({ workspaceId: "alpha", registeredWorkspaceId: "alpha" });
    });

    await waitFor(() => expect(result.current.state.worktrees).toHaveLength(1));
    expect(result.current.state.worktrees[0].path).toBe("/repos/alpha");
  });

  it("keeps a project's worktrees when a slow close from the previous project resolves after the switch", async () => {
    let releaseClose: (() => void) | null = null;
    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(async () => "backend-1"),
      getTerminalCwd: vi.fn(async (path: string) => path),
      closeTerminal: vi.fn(
        () =>
          new Promise<void>((resolve) => {
            releaseClose = () => resolve();
          }),
      ),
      waitForTerminalExit: vi.fn(async () => undefined),
    };

    const { result, rerender } = renderHook(
      ({ workspaceId, initialWorktrees }: { workspaceId: string; initialWorktrees: Worktree[] }) =>
        useWorkspaceStore({ workspaceId, initialWorktrees, services }),
      { initialProps: { workspaceId: "alpha", initialWorktrees: [alphaMain] } },
    );

    await act(async () => {
      await result.current.openTab(alphaMain);
    });
    const alphaTabId = result.current.state.layout.tabs[0].id;

    let closing!: Promise<void>;
    act(() => {
      closing = result.current.closeTab(alphaTabId) as Promise<void>;
    });

    await act(async () => {
      rerender({ workspaceId: "beta", initialWorktrees: [betaMain] });
    });
    expect(result.current.state.worktrees[0].path).toBe("/repos/beta");

    await act(async () => {
      releaseClose?.();
      await closing;
    });

    expect(result.current.state.workspaceId).toBe("beta");
    expect(result.current.state.worktrees.map((worktree) => worktree.path)).toEqual(["/repos/beta"]);

    await act(async () => {
      rerender({ workspaceId: "alpha", initialWorktrees: [alphaMain] });
    });

    expect(result.current.state.workspaceId).toBe("alpha");
    expect(result.current.state.worktrees.map((worktree) => worktree.path)).toEqual(["/repos/alpha"]);
  });

  it("applies the first worktree sync that arrives right after a project switch", async () => {
    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(async () => "backend-1"),
      getTerminalCwd: vi.fn(async (path: string) => path),
      closeTerminal: vi.fn(async () => undefined),
      waitForTerminalExit: vi.fn(async () => undefined),
    };

    const { result, rerender } = renderHook(
      ({ workspaceId }: { workspaceId: string }) => useWorkspaceStore({ workspaceId, services }),
      { initialProps: { workspaceId: "alpha" } },
    );

    await act(async () => {
      await result.current.syncWorktrees([alphaMain]);
    });
    expect(result.current.state.worktrees.map((w) => w.path)).toEqual(["/repos/alpha"]);

    const syncForBeta = result.current.syncWorktrees;
    await act(async () => {
      rerender({ workspaceId: "beta" });
    });

    await act(async () => {
      await syncForBeta([betaMain]);
    });

    expect(result.current.state.worktrees.map((w) => w.path)).toEqual(["/repos/beta"]);
  });

  it("keeps the plain-folder root row after switching to a git project and back", async () => {
    const plainRoot: Worktree = {
      path: "/repos/plain",
      head: "",
      branch: null,
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    const listed: Record<string, Worktree[]> = { plain: [], alpha: [alphaMain] };
    const runtimeServices: WorkspaceRuntimeServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      listWorktrees: vi.fn(async (workspaceId: string) => listed[workspaceId] ?? []),
      onWorktreeChanged: vi.fn(async () => () => undefined),
      isTauriRuntime: vi.fn(() => true),
    };

    function useShell(workspaceId: string, plain: Worktree | null) {
      const store = useWorkspaceStore({ workspaceId, services: storeServices });
      useWorkspaceRuntime({
        workspaceId,
        activeWorktreePath: store.state.activeWorktreePath,
        syncWorktrees: store.syncWorktrees,
        ensureTabForWorktree: store.ensureTabForWorktree,
        plainRootWorktree: plain,
        registeredWorkspaceId: workspaceId,
        services: runtimeServices,
      });
      return store;
    }

    const { result, rerender } = renderHook(
      ({ workspaceId, plain }: { workspaceId: string; plain: Worktree | null }) => useShell(workspaceId, plain),
      { initialProps: { workspaceId: "plain", plain: plainRoot as Worktree | null } },
    );

    await waitFor(() => expect(result.current.state.worktrees.map((w) => w.path)).toEqual(["/repos/plain"]));

    await act(async () => {
      rerender({ workspaceId: "alpha", plain: null });
    });
    await waitFor(() => expect(result.current.state.worktrees.map((w) => w.path)).toEqual(["/repos/alpha"]));

    await act(async () => {
      rerender({ workspaceId: "plain", plain: plainRoot });
    });

    await waitFor(() => expect(result.current.state.worktrees.map((w) => w.path)).toEqual(["/repos/plain"]));
  });

  it("closes a backend session that was spawned for a workspace replaced mid-flight", async () => {
    let releaseSpawn: ((id: string) => void) | null = null;
    const closeTerminal = vi.fn(async () => undefined);
    const services: WorkspaceServices = {
      ensureTerminalEvents: vi.fn(async () => undefined),
      spawnTerminal: vi.fn(
        () =>
          new Promise<string>((resolve) => {
            releaseSpawn = resolve;
          }),
      ),
      getTerminalCwd: vi.fn(async (path: string) => path),
      closeTerminal,
      waitForTerminalExit: vi.fn(async () => undefined),
    };

    const { result, rerender } = renderHook(
      ({ workspaceId, initialWorktrees }: { workspaceId: string; initialWorktrees: Worktree[] }) =>
        useWorkspaceStore({ workspaceId, initialWorktrees, services }),
      { initialProps: { workspaceId: "alpha", initialWorktrees: [alphaMain] } },
    );

    let opened!: Promise<string | null>;
    act(() => {
      opened = result.current.openTab(alphaMain) as Promise<string | null>;
    });
    await waitFor(() => expect(releaseSpawn).not.toBeNull());

    await act(async () => {
      rerender({ workspaceId: "beta", initialWorktrees: [betaMain] });
    });

    await act(async () => {
      releaseSpawn?.("orphan-backend");
      await opened;
    });

    expect(await opened).toBeNull();
    expect(result.current.state.layout.tabs).toHaveLength(0);
    expect(closeTerminal).toHaveBeenCalledWith("orphan-backend");
  });

  it("reports recovery provenance for the workspace it swapped to, not the first one", async () => {
    const { result, rerender } = renderHook(
      ({ workspaceId, initialWorktrees }: { workspaceId: string; initialWorktrees: Worktree[] }) =>
        useWorkspaceStore({ workspaceId, initialWorktrees, services: storeServices }),
      { initialProps: { workspaceId: "alpha", initialWorktrees: [alphaMain] } },
    );

    expect(result.current.recoveredFromHmr).toBe(false);

    await act(async () => {
      await result.current.openTab(alphaMain);
    });

    await act(async () => {
      rerender({ workspaceId: "beta", initialWorktrees: [betaMain] });
    });
    expect(result.current.recoveredFromHmr).toBe(false);

    await act(async () => {
      rerender({ workspaceId: "alpha", initialWorktrees: [alphaMain] });
    });

    expect(result.current.recoveredFromHmr).toBe(true);
  });

  it("closes a browser created for a workspace that was replaced mid-flight", async () => {
    let releaseBrowser: (state: unknown) => void = () => undefined;
    createBrowserMock.mockImplementation(
      () =>
        new Promise((resolve) => {
          releaseBrowser = resolve;
        }),
    );

    const { result, rerender } = renderHook(
      ({ workspaceId, initialWorktrees }: { workspaceId: string; initialWorktrees: Worktree[] }) =>
        useWorkspaceStore({ workspaceId, initialWorktrees, services: storeServices }),
      { initialProps: { workspaceId: "alpha", initialWorktrees: [alphaMain] } },
    );

    let pending: Promise<string | null> | undefined;
    act(() => {
      pending = result.current.createBrowserTab("http://localhost:3000") as Promise<string | null>;
    });

    await act(async () => {
      rerender({ workspaceId: "beta", initialWorktrees: [betaMain] });
    });

    await act(async () => {
      releaseBrowser({
        browserId: "browser-late",
        url: "http://localhost:3000",
        title: "t",
        loading: false,
        canGoBack: false,
        canGoForward: false,
      });
      await pending;
    });

    expect(closeBrowserMock).toHaveBeenCalledWith("browser-late");
    expect(
      result.current.state.layout.tabs.some(
        (tab) => tab.kind === "browser" && tab.browserId === "browser-late",
      ),
    ).toBe(false);
  });

});
