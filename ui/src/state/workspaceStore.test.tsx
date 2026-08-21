import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const featureWorktree: Worktree = {
  ...worktree,
  path: "/repo/feature",
  branch: "refs/heads/orca/ws-main/feature",
};

type ExitDeferred = {
  promise: Promise<void>;
  resolve: () => void;
};

function createServices({ autoConfirmExit = true }: { autoConfirmExit?: boolean } = {}) {
  let sessionNumber = 0;
  const activeByWorktree = new Map<string, string>();
  const worktreeBySession = new Map<string, string>();
  const exits = new Map<string, ExitDeferred>();

  const getExit = (sessionId: string) => {
    const existing = exits.get(sessionId);
    if (existing) return existing;
    let resolve!: () => void;
    const promise = new Promise<void>((done) => {
      resolve = done;
    });
    const deferred = { promise, resolve };
    exits.set(sessionId, deferred);
    return deferred;
  };

  const confirmExit = (sessionId: string) => {
    const key = worktreeBySession.get(sessionId);
    if (key && activeByWorktree.get(key) === sessionId) activeByWorktree.delete(key);
    worktreeBySession.delete(sessionId);
    const deferred = getExit(sessionId);
    exits.delete(sessionId);
    deferred.resolve();
  };

  const services: WorkspaceServices = {
    ensureTerminalEvents: vi.fn(async () => undefined),
    spawnTerminal: vi.fn(async (request) => {
      const key = `${request.workspaceId}:${request.worktree?.wsId ?? "root"}:${request.worktree?.slug ?? "root"}`;
      if (activeByWorktree.has(key)) {
        throw { code: "WRITER_ALREADY_ACTIVE", message: "writer already active", details: { worktree: request.worktree } };
      }
      const sessionId = `backend-${++sessionNumber}`;
      activeByWorktree.set(key, sessionId);
      worktreeBySession.set(sessionId, key);
      return sessionId;
    }),
    closeTerminal: vi.fn(async (sessionId) => {
      const deferred = getExit(sessionId);
      if (autoConfirmExit) queueMicrotask(() => confirmExit(sessionId));
      await deferred.promise;
    }),
    waitForTerminalExit: vi.fn(async (sessionId) => {
      await getExit(sessionId).promise;
    }),
  };

  return { services, confirmExit };
}

describe("useWorkspaceStore terminal ownership", () => {
  it("splits a pane in a tab tree using existing sessions without spawning another PTY", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    await act(async () => {
      await result.current.openTab(worktree);
    });
    vi.mocked(services.spawnTerminal).mockClear();

    const tabId = result.current.state.layout.activeTabId!;
    const tabLayout = result.current.state.layout.layoutsByTabId[tabId];
    const leafId = tabLayout.activeLeafId!;

    await act(async () => {
      await result.current.splitPane(tabId, leafId, "horizontal");
    });

    expect(result.current.state.layout.tabs).toHaveLength(1);
    const updatedLayout = result.current.state.layout.layoutsByTabId[tabId];
    expect(updatedLayout.root.type).toBe("split");
    expect(services.spawnTerminal).not.toHaveBeenCalled();
  });

  it("closes individual panes in a tab layout", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    await act(async () => {
      await result.current.openTab(worktree);
    });

    const tabId = result.current.state.layout.activeTabId!;
    const leafId = result.current.state.layout.layoutsByTabId[tabId].activeLeafId!;

    await act(async () => {
      await result.current.splitPane(tabId, leafId, "horizontal");
    });

    const splitLayout = result.current.state.layout.layoutsByTabId[tabId];
    const newLeafId = splitLayout.activeLeafId!;

    await act(async () => {
      await result.current.closePane(tabId, newLeafId);
    });

    const finalLayout = result.current.state.layout.layoutsByTabId[tabId];
    expect(finalLayout.root.type).toBe("leaf");
  });

  it("spawns a last-tab replacement only after lifecycle-confirmed writer release", async () => {
    const { services, confirmExit } = createServices({ autoConfirmExit: false });
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    await act(async () => {
      await result.current.openTab(worktree);
    });
    const closingTabId = result.current.state.layout.activeTabId!;
    const closingTab = result.current.state.layout.tabs[0];
    const closingSessionId = closingTab.kind === "browser" ? "" : closingTab.sessionId;
    const closingBackendId = result.current.state.sessions[closingSessionId].backendSessionId!;

    const closePromise = result.current.closeTab(closingTabId);
    await vi.waitFor(() => expect(services.closeTerminal).toHaveBeenCalledWith(closingBackendId));

    expect(services.waitForTerminalExit).toHaveBeenCalledWith(closingBackendId, 5_000);
    expect(services.spawnTerminal).toHaveBeenCalledTimes(1);
    expect(vi.mocked(services.waitForTerminalExit).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(services.closeTerminal).mock.invocationCallOrder[0],
    );

    confirmExit(closingBackendId);
    await act(async () => {
      await closePromise;
    });

    expect(result.current.state.layout.tabs).toHaveLength(1);
    expect(result.current.state.layout.activeTabId).toBe(result.current.state.layout.tabs[0].id);
    expect(result.current.state.layout.tabs[0].id).not.toBe(closingTabId);
    expect(services.spawnTerminal).toHaveBeenCalledTimes(2);
    expect(vi.mocked(services.closeTerminal).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(services.spawnTerminal).mock.invocationCallOrder[1],
    );
  });

  it("derives visible agents only from live terminal session metadata", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    expect(result.current.agents).toEqual([]);
    await act(async () => {
      await result.current.openTab(worktree);
    });

    expect(result.current.agents).toEqual([
      expect.objectContaining({
        id: "backend-1",
        sessionId: "backend-1",
        state: "working",
        worktree: { wsId: "ws-main", slug: "main" },
        worktreePath: worktree.path,
        task: "orca/ws-main/main",
      }),
    ]);
  });

  it("removes deleted-worktree tabs, sessions, and agents during synchronization", async () => {
    const { services } = createServices();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree, featureWorktree], services }),
    );

    await act(async () => {
      await result.current.openTab(worktree);
      await result.current.openTab(featureWorktree);
    });
    expect(result.current.agents).toHaveLength(2);

    await act(async () => {
      await result.current.syncWorktrees([worktree]);
    });

    expect(result.current.state.worktrees).toEqual([worktree]);
    expect(Object.values(result.current.state.sessions).every((session) => session.cwd === worktree.path)).toBe(true);
    expect(result.current.state.layout.tabs).toHaveLength(1);
    expect(result.current.agents).toHaveLength(1);
    expect(services.closeTerminal).toHaveBeenCalledWith("backend-2");
  });
});
