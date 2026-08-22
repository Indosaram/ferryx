import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { TerminalSession, TerminalTab, Worktree } from "../lib/types";
import { terminalHostManager } from "../lib/terminalHostManager";
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
      const sessionId = `backend-${++sessionNumber}`;
      activeByWorktree.set(key, sessionId);
      worktreeBySession.set(sessionId, key);
      return sessionId;
    }),
    getTerminalCwd: vi.fn(async () => "/repo/main/packages/api"),
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

function restoredTwoTabState(activeTabId = "tab-primary"): WorkspaceState {
  const primary: TerminalTab = { id: "tab-primary", label: "primary", sessionId: "session-1" };
  const secondary: TerminalTab = { id: "tab-secondary", label: "secondary", sessionId: "session-2" };
  return {
    worktrees: [worktree],
    activeWorktreePath: worktree.path,
    sessions: {
      "session-1": restoredSession("session-1", "restored-backend-1"),
      "session-2": restoredSession("session-2", "restored-backend-2"),
    },
    layout: {
      tabs: [primary, secondary],
      activeTabId,
      tabGroups: {
        "group-default": {
          id: "group-default",
          tabIds: [primary.id, secondary.id],
          activeTabId,
        },
      },
      tabGroupLayout: { type: "group", groupId: "group-default" },
      focusedGroupId: "group-default",
      layoutsByTabId: {
        [primary.id]: {
          root: { type: "leaf", leafId: "leaf-primary" },
          activeLeafId: "leaf-primary",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-primary": "session-1" },
        },
        [secondary.id]: {
          root: { type: "leaf", leafId: "leaf-secondary" },
          activeLeafId: "leaf-secondary",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-secondary": "session-2" },
        },
      },
    },
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  };
}

function groupForTab(state: WorkspaceState, tabId: string) {
  return Object.values(state.layout.tabGroups ?? {}).find((group) => group.tabIds.includes(tabId));
}

describe("useWorkspaceStore terminal ownership", () => {
  it("splits a pane by creating an independent backend PTY and local session", async () => {
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
    expect(services.spawnTerminal).toHaveBeenCalledTimes(1);
    const sessionIds = Object.values(updatedLayout.sessionIdsByLeafId);
    expect(new Set(sessionIds).size).toBe(2);
    const newSession = result.current.state.sessions[updatedLayout.sessionIdsByLeafId[updatedLayout.activeLeafId!]];
    expect(newSession.backendSessionId).toBe("backend-2");
    expect(newSession.cwd).toBe("/repo/main/packages/api");
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
    expect(layout.sessionIdsByLeafId[newLeafId]).not.toBe("session-2");
    const splitSession = result.current.state.sessions[layout.sessionIdsByLeafId[newLeafId]];
    expect(splitSession.backendSessionId).toBe("backend-1");
    expect(splitSession.cwd).toBe("/repo/main/packages/api");
  });

  it("moves a dragged tab into a new split group without closing its PTY, matching Orca dropUnifiedTab", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));
    act(() => result.current.restoreWorkspace(restoredTwoTabState()));

    await act(async () => {
      const targetGroupId = groupForTab(result.current.state, "tab-primary")!.id;
      result.current.moveTabToSplit("tab-secondary", targetGroupId, "horizontal", "second");
    });

    expect(new Set(result.current.state.layout.tabs.map((tab) => tab.id))).toEqual(
      new Set(["tab-primary", "tab-secondary"]),
    );
    expect(result.current.state.layout.activeTabId).toBe("tab-secondary");
    expect(result.current.state.layout.layoutsByTabId["tab-primary"].root).toEqual({ type: "leaf", leafId: "leaf-primary" });
    expect(result.current.state.layout.layoutsByTabId["tab-secondary"].root).toEqual({ type: "leaf", leafId: "leaf-secondary" });

    const primaryGroup = groupForTab(result.current.state, "tab-primary");
    const secondaryGroup = groupForTab(result.current.state, "tab-secondary");
    expect(primaryGroup?.id).not.toBe(secondaryGroup?.id);
    expect(result.current.state.layout.tabGroupLayout).toMatchObject({
      type: "split",
      direction: "horizontal",
      first: { type: "group", groupId: primaryGroup?.id },
      second: { type: "group", groupId: secondaryGroup?.id },
    });
    expect(result.current.state.sessions["session-1"]).toBeDefined();
    expect(result.current.state.sessions["session-2"]).toBeDefined();
    expect(services.closeTerminal).not.toHaveBeenCalled();
  });

  it("moves the active tab into its own split group when another terminal tab exists, matching Orca same-group behavior", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));
    act(() => result.current.restoreWorkspace(restoredTwoTabState("tab-primary")));

    await act(async () => {
      const targetGroupId = groupForTab(result.current.state, "tab-primary")!.id;
      result.current.moveTabToSplit("tab-primary", targetGroupId, "vertical", "first");
    });

    expect(new Set(result.current.state.layout.tabs.map((tab) => tab.id))).toEqual(
      new Set(["tab-primary", "tab-secondary"]),
    );
    expect(result.current.state.layout.activeTabId).toBe("tab-primary");
    expect(result.current.state.layout.layoutsByTabId["tab-primary"].root).toEqual({ type: "leaf", leafId: "leaf-primary" });
    expect(result.current.state.layout.layoutsByTabId["tab-secondary"].root).toEqual({ type: "leaf", leafId: "leaf-secondary" });

    const primaryGroup = groupForTab(result.current.state, "tab-primary");
    const secondaryGroup = groupForTab(result.current.state, "tab-secondary");
    expect(primaryGroup?.id).not.toBe(secondaryGroup?.id);
    expect(result.current.state.layout.tabGroupLayout).toMatchObject({
      type: "split",
      direction: "vertical",
      first: { type: "group", groupId: primaryGroup?.id },
      second: { type: "group", groupId: secondaryGroup?.id },
    });
    expect(result.current.state.sessions["session-1"]).toBeDefined();
    expect(result.current.state.sessions["session-2"]).toBeDefined();
    expect(services.closeTerminal).not.toHaveBeenCalled();
  });

  it("keeps a sole active terminal tab unsplit when it is dragged onto itself, matching Orca's no-op guard", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    await act(async () => {
      await result.current.openTab(worktree);
    });
    const tabId = result.current.state.layout.activeTabId!;

    await act(async () => {
      const targetGroupId = groupForTab(result.current.state, tabId)!.id;
      result.current.moveTabToSplit(tabId, targetGroupId, "horizontal", "first");
    });

    expect(result.current.state.layout.tabs.map((tab) => tab.id)).toEqual([tabId]);
    expect(result.current.state.layout.layoutsByTabId[tabId].root.type).toBe("leaf");
    expect(services.closeTerminal).not.toHaveBeenCalled();
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

describe("session title activity", () => {
  it("clears the working indicator when the title carries no activity signal", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    let tabId!: string;
    await act(async () => {
      tabId = await result.current.openTab(worktree);
    });
    const workingTab = result.current.state.layout.tabs.find((tab) => tab.id === tabId);
    if (!workingTab || workingTab.kind === "browser") throw new Error("terminal tab expected");
    const sessionId = workingTab.sessionId;

    act(() => {
      result.current.updateSessionTitleActivity(tabId, "⠹ agy: running task");
    });
    expect(result.current.state.activityBySessionId?.[sessionId]?.state).toBe("working");

    act(() => {
      result.current.updateSessionTitleActivity(tabId, "agy");
    });
    expect(result.current.state.activityBySessionId?.[sessionId]).toBeUndefined();
  });

  it("does not create an indicator for a bare agent-name title", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    let tabId!: string;
    await act(async () => {
      tabId = await result.current.openTab(worktree);
    });
    const idleTab = result.current.state.layout.tabs.find((tab) => tab.id === tabId);
    if (!idleTab || idleTab.kind === "browser") throw new Error("terminal tab expected");
    const sessionId = idleTab.sessionId;

    act(() => {
      result.current.updateSessionTitleActivity(tabId, "agy");
    });
    expect(result.current.state.activityBySessionId?.[sessionId]).toBeUndefined();
  });

  it("invokes terminalHostManager.destroy when closing a tab", async () => {
    const destroySpy = vi.spyOn(terminalHostManager, "destroy");
    const { services } = createServices();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree, featureWorktree], services }),
    );

    let tab1Id!: string;
    await act(async () => {
      tab1Id = await result.current.openTab(worktree);
      await result.current.openTab(featureWorktree);
    });

    const tab1 = result.current.state.layout.tabs.find((t) => t.id === tab1Id);
    if (!tab1 || tab1.kind === "browser") throw new Error("terminal tab expected");
    const session1Id = tab1.sessionId;

    destroySpy.mockClear();
    await act(async () => {
      await result.current.closeTab(tab1Id);
    });

    expect(destroySpy).toHaveBeenCalledWith(session1Id);
    destroySpy.mockRestore();
  });

  it("invokes terminalHostManager.destroy when closing a pane", async () => {
    const destroySpy = vi.spyOn(terminalHostManager, "destroy");
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    act(() => result.current.restoreWorkspace(restoredSplitState()));

    destroySpy.mockClear();
    await act(async () => {
      await result.current.closePane("tab-primary", "leaf-2");
    });

    expect(destroySpy).toHaveBeenCalledWith("session-2");
    destroySpy.mockRestore();
  });

  it("invokes terminalHostManager.destroy when closing a sole tab (which creates a replacement)", async () => {
    const destroySpy = vi.spyOn(terminalHostManager, "destroy");
    const { services } = createServices();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree], services }),
    );

    let tabId!: string;
    await act(async () => {
      tabId = await result.current.openTab(worktree);
    });

    const tab = result.current.state.layout.tabs.find((t) => t.id === tabId);
    if (!tab || tab.kind === "browser") throw new Error("terminal tab expected");
    const sessionId = tab.sessionId;

    destroySpy.mockClear();
    await act(async () => {
      await result.current.closeTab(tabId);
    });

    expect(destroySpy).toHaveBeenCalledWith(sessionId);
    destroySpy.mockRestore();
  });
});
