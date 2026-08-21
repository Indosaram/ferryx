import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { TerminalSession, TerminalTab, Worktree } from "../lib/types";
import { useWorkspaceStore, type WorkspaceServices, type WorkspaceState } from "./workspaceStore";

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

const docsWorktree: Worktree = {
  ...worktree,
  path: "/repo/docs",
  branch: "refs/heads/orca/ws-main/docs",
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

function restoredSession(id: string, backendSessionId: string, cwd = worktree.path): TerminalSession {
  return {
    id,
    cwd,
    workspaceId: "ws-main",
    worktree: cwd === worktree.path ? { wsId: "ws-main", slug: "main" } : { wsId: "ws-main", slug: "feature" },
    backendSessionId,
    lifecycle: "working",
  };
}

function restoredSplitState(): WorkspaceState {
  const primary: TerminalTab = { id: "tab-primary", label: "primary", sessionId: "session-1" };
  return {
    worktrees: [worktree, featureWorktree],
    activeWorktreePath: worktree.path,
    sessions: {
      "session-1": restoredSession("session-1", "restored-backend-1"),
      "session-2": restoredSession("session-2", "restored-backend-2"),
    },
    layout: {
      tabs: [primary],
      activeTabId: primary.id,
      layoutsByTabId: {
        [primary.id]: {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-1" },
            second: { type: "leaf", leafId: "leaf-2" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-2",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "session-1", "leaf-2": "session-2" },
        },
      },
    },
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  };
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

  it("uses the target leaf session when splitting restored independent panes", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    act(() => result.current.restoreWorkspace(restoredSplitState()));
    await act(async () => {
      await result.current.splitPane("tab-primary", "leaf-2", "vertical");
    });

    const layout = result.current.state.layout.layoutsByTabId["tab-primary"];
    const newLeafId = layout.activeLeafId!;
    expect(newLeafId).not.toBe("leaf-2");
    expect(layout.sessionIdsByLeafId[newLeafId]).toBe("session-2");
    expect(services.spawnTerminal).not.toHaveBeenCalled();
  });

  it("keeps a split session alive while referenced and closes it after its final leaf is removed", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    act(() => result.current.restoreWorkspace(restoredSplitState()));
    await act(async () => {
      await result.current.splitPane("tab-primary", "leaf-2", "vertical");
    });
    const duplicatedLeafId = result.current.state.layout.layoutsByTabId["tab-primary"].activeLeafId!;

    await act(async () => {
      await result.current.closePane("tab-primary", duplicatedLeafId);
    });
    expect(services.closeTerminal).not.toHaveBeenCalledWith("restored-backend-2");
    expect(result.current.state.sessions["session-2"]).toBeDefined();

    await act(async () => {
      await result.current.closePane("tab-primary", "leaf-2");
    });
    expect(services.closeTerminal).toHaveBeenCalledWith("restored-backend-2");
    expect(result.current.state.sessions["session-2"]).toBeUndefined();
  });

  it("promotes a surviving pane session when the tab's primary leaf is closed", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    act(() => result.current.restoreWorkspace(restoredSplitState()));
    await act(async () => {
      await result.current.closePane("tab-primary", "leaf-1");
    });

    const tab = result.current.state.layout.tabs[0];
    expect(tab.kind).not.toBe("browser");
    if (tab.kind !== "browser") expect(tab.sessionId).toBe("session-2");
    expect(result.current.state.sessions["session-1"]).toBeUndefined();
    expect(services.closeTerminal).toHaveBeenCalledWith("restored-backend-1");
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

  it("reorders, renames, pins, and bulk-closes tabs without closing pinned tabs", async () => {
    const { services } = createServices();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree, featureWorktree, docsWorktree], services }),
    );

    let mainTabId = "";
    let featureTabId = "";
    let docsTabId = "";
    await act(async () => {
      mainTabId = await result.current.openTab(worktree);
      featureTabId = await result.current.openTab(featureWorktree);
      docsTabId = await result.current.openTab(docsWorktree);
    });

    act(() => {
      result.current.renameTab(mainTabId, "  renamed main  ");
      result.current.setTabPinned(featureTabId, true);
      result.current.reorderTab(docsTabId, 0);
    });

    expect(result.current.state.layout.tabs.map((tab) => tab.id)).toEqual([docsTabId, mainTabId, featureTabId]);
    expect(result.current.state.layout.tabs.find((tab) => tab.id === mainTabId)?.label).toBe("renamed main");
    expect(result.current.state.layout.tabs.find((tab) => tab.id === featureTabId)?.pinned).toBe(true);

    await act(async () => {
      await result.current.closeTabsToRight(docsTabId);
    });
    expect(result.current.state.layout.tabs.map((tab) => tab.id)).toEqual([docsTabId, featureTabId]);
    expect(services.closeTerminal).toHaveBeenCalledWith("backend-1");

    vi.mocked(services.closeTerminal).mockClear();
    await act(async () => {
      await result.current.closeTab(featureTabId);
    });
    expect(result.current.state.layout.tabs.some((tab) => tab.id === featureTabId)).toBe(true);
    expect(services.closeTerminal).not.toHaveBeenCalled();
  });

  it("closes every unshared backend session owned by a tab", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree, featureWorktree], services }));
    const state = restoredSplitState();
    const secondary: TerminalTab = { id: "tab-secondary", label: "secondary", sessionId: "session-3" };
    state.sessions["session-3"] = restoredSession("session-3", "restored-backend-3", featureWorktree.path);
    state.layout.tabs.push(secondary);
    state.layout.layoutsByTabId[secondary.id] = {
      root: { type: "leaf", leafId: "leaf-3" },
      activeLeafId: "leaf-3",
      expandedLeafId: null,
      sessionIdsByLeafId: { "leaf-3": "session-3" },
    };

    act(() => result.current.restoreWorkspace(state));
    await act(async () => {
      await result.current.closeTab("tab-primary");
    });

    expect(result.current.state.layout.tabs.map((tab) => tab.id)).toEqual(["tab-secondary"]);
    expect(result.current.state.sessions["session-1"]).toBeUndefined();
    expect(result.current.state.sessions["session-2"]).toBeUndefined();
    expect(services.closeTerminal).toHaveBeenCalledWith("restored-backend-1");
    expect(services.closeTerminal).toHaveBeenCalledWith("restored-backend-2");
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
