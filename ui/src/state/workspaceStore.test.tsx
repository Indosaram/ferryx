import { JSDOM } from "jsdom";

if (typeof window === "undefined") {
  const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>", { url: "http://localhost:3000" });
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  globalThis.navigator = dom.window.navigator;
  globalThis.localStorage = dom.window.localStorage;
  globalThis.sessionStorage = dom.window.sessionStorage;
  globalThis.HTMLElement = dom.window.HTMLElement;
}

import { StrictMode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { BrowserTab, LayoutState, TerminalSession, TerminalTab, Worktree } from "../lib/types";
import * as browserTauri from "../lib/browserTauri";
import { createLayoutState } from "./layout";
import { clearHmrWorkspaceState } from "./hmrWorkspaceState";
import { clearWorkspaceSnapshot, setWorkspaceSnapshot } from "./workspaceSnapshotCache";
const { useWorkspaceStore, workspaceReducer } = await import("./workspaceStore");
type WorkspaceServices = import("./workspaceStore").WorkspaceServices;
type WorkspaceState = import("./workspaceStore").WorkspaceState;

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

function openedTab(tabId: string | null): string {
  if (tabId === null) throw new Error("expected openTab to return a tab id");
  return tabId;
}

afterEach(() => {
  clearHmrWorkspaceState();
  clearWorkspaceSnapshot();
});

describe("useWorkspaceStore terminal ownership", () => {
  it("splits a pane by creating an independent backend PTY and local session", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    await act(async () => {
      await result.current.openTab(worktree);
    });
    (services.spawnTerminal as any).mockClear();

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

  it("dispatches split layout immediately while deferred spawn is unresolved, then rebinds when spawn resolves", async () => {
    let resolveDeferredSpawn!: (backendId: string) => void;
    const deferredSpawnPromise = new Promise<string>((resolve) => {
      resolveDeferredSpawn = resolve;
    });

    const { services } = createServices();
    (services.spawnTerminal as any).mockImplementation(async () => deferredSpawnPromise);
    (services.getTerminalCwd as any).mockImplementation(async () => "/repo/main/resolved-cwd");
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    act(() => result.current.restoreWorkspace(restoredSplitState()));

    const initialLayout = result.current.state.layout.layoutsByTabId["tab-primary"];
    expect(initialLayout.root.type).toBe("split");

    let splitPromise!: Promise<void>;
    act(() => {
      splitPromise = result.current.splitPane("tab-primary", "leaf-2", "vertical");
    });

    const inFlightLayout = result.current.state.layout.layoutsByTabId["tab-primary"];
    const inFlightLeafId = inFlightLayout.activeLeafId!;
    expect(inFlightLeafId).not.toBe("leaf-2");
    const inFlightSessionId = inFlightLayout.sessionIdsByLeafId[inFlightLeafId];
    expect(inFlightSessionId).toBeDefined();

    const inFlightSession = result.current.state.sessions[inFlightSessionId];
    expect(inFlightSession).toBeDefined();
    expect(inFlightSession.backendSessionId).toBeNull();
    expect(inFlightSession.cwd).toBe(worktree.path);

    await act(async () => {
      resolveDeferredSpawn("backend-deferred-split");
      await splitPromise;
    });

    const reboundSession = result.current.state.sessions[inFlightSessionId];
    expect(reboundSession).toBeDefined();
    expect(reboundSession.backendSessionId).toBe("backend-deferred-split");
    expect(reboundSession.cwd).toBe("/repo/main/resolved-cwd");
    expect(reboundSession.lifecycle).toBe("running");
  });

  it("closes deferred spawned backend session if the pane is closed before spawn resolves", async () => {
    let resolveDeferredSpawn!: (backendId: string) => void;
    const deferredSpawnPromise = new Promise<string>((resolve) => {
      resolveDeferredSpawn = resolve;
    });

    const { services } = createServices();
    (services.spawnTerminal as any).mockImplementation(async () => deferredSpawnPromise);

    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));
    act(() => result.current.restoreWorkspace(restoredSplitState()));

    let splitPromise!: Promise<void>;
    act(() => {
      splitPromise = result.current.splitPane("tab-primary", "leaf-2", "vertical");
    });

    const layout = result.current.state.layout.layoutsByTabId["tab-primary"];
    const inFlightLeafId = layout.activeLeafId!;
    expect(inFlightLeafId).not.toBe("leaf-2");

    await act(async () => {
      await result.current.closePane("tab-primary", inFlightLeafId);
    });

    await act(async () => {
      resolveDeferredSpawn("backend-orphaned");
      await splitPromise;
    });

    expect(services.closeTerminal).toHaveBeenCalledWith("backend-orphaned");
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
      mainTabId = openedTab(await result.current.openTab(worktree, "main"));
      featureTabId = openedTab(await result.current.openTab(worktree, "feature"));
      docsTabId = openedTab(await result.current.openTab(worktree, "docs"));
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

    (services.closeTerminal as any).mockClear();
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

  it("closes the last tab only after lifecycle-confirmed writer release", async () => {
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
    await waitFor(() => expect(services.closeTerminal).toHaveBeenCalledWith(closingBackendId));

    expect(services.waitForTerminalExit).toHaveBeenCalledWith(closingBackendId, 5_000);
    expect(services.spawnTerminal).toHaveBeenCalledTimes(1);
    expect((services.waitForTerminalExit as any).mock.invocationCallOrder[0]).toBeLessThan(
      (services.closeTerminal as any).mock.invocationCallOrder[0],
    );

    confirmExit(closingBackendId);
    await act(async () => {
      await closePromise;
    });

    expect(result.current.state.layout.tabs).toHaveLength(0);
    expect(result.current.state.layout.activeTabId).toBeNull();
    expect(services.spawnTerminal).toHaveBeenCalledTimes(1);
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
      tabId = openedTab(await result.current.openTab(worktree));
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
      tabId = openedTab(await result.current.openTab(worktree));
    });
    const idleTab = result.current.state.layout.tabs.find((tab) => tab.id === tabId);
    if (!idleTab || idleTab.kind === "browser") throw new Error("terminal tab expected");
    const sessionId = idleTab.sessionId;

    act(() => {
      result.current.updateSessionTitleActivity(tabId, "agy");
    });
    expect(result.current.state.activityBySessionId?.[sessionId]).toBeUndefined();
  });

  it("closes the backend session when closing a tab", async () => {
    const { services } = createServices();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree, featureWorktree], services }),
    );

    let tab1Id!: string;
    await act(async () => {
      tab1Id = openedTab(await result.current.openTab(worktree, "tab-1"));
      await result.current.openTab(worktree, "tab-2");
    });

    const tab1 = result.current.state.layout.tabs.find((t) => t.id === tab1Id);
    if (!tab1 || tab1.kind === "browser") throw new Error("terminal tab expected");
    const backendId = result.current.state.sessions[tab1.sessionId]?.backendSessionId;

    await act(async () => {
      await result.current.closeTab(tab1Id);
    });

    expect(services.closeTerminal).toHaveBeenCalledWith(backendId);
  });

  it("closes the backend session when closing a pane", async () => {
    const { services } = createServices();
    const { result } = renderHook(() => useWorkspaceStore({ initialWorktrees: [worktree], services }));

    act(() => result.current.restoreWorkspace(restoredSplitState()));

    await act(async () => {
      await result.current.closePane("tab-primary", "leaf-2");
    });

    expect(services.closeTerminal).toHaveBeenCalledWith("restored-backend-2");
  });

  it("closes the backend session when closing a sole tab", async () => {
    const { services } = createServices();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree], services }),
    );

    let tabId!: string;
    await act(async () => {
      tabId = openedTab(await result.current.openTab(worktree));
    });

    const tab = result.current.state.layout.tabs.find((t) => t.id === tabId);
    if (!tab || tab.kind === "browser") throw new Error("terminal tab expected");
    const backendId = result.current.state.sessions[tab.sessionId]?.backendSessionId;

    await act(async () => {
      await result.current.closeTab(tabId);
    });

    expect(services.closeTerminal).toHaveBeenCalledWith(backendId);
  });
});

describe("worktree tab and session isolation", () => {
  it("never exposes the outgoing workspace state under the incoming workspace id", () => {
    const { services } = createServices();
    const alphaState: WorkspaceState = {
      ...restoredTwoTabState(),
      workspaceId: "alpha",
    };
    const betaState: WorkspaceState = {
      ...restoredTwoTabState(),
      workspaceId: "beta",
      worktrees: [featureWorktree],
      activeWorktreePath: featureWorktree.path,
      sessions: Object.fromEntries(
        Object.entries(restoredTwoTabState().sessions).map(([id, session]) => [
          id,
          {
            ...session,
            cwd: featureWorktree.path,
            worktreePath: featureWorktree.path,
            workspaceId: "beta",
            worktree: { wsId: "ws-main", slug: "feature" },
          },
        ]),
      ),
    };
    setWorkspaceSnapshot("alpha", alphaState);
    setWorkspaceSnapshot("beta", betaState);

    const observed: Array<{
      requestedWorkspaceId: string;
      stateWorkspaceId: string | undefined;
      activeWorktreePath: string | null;
    }> = [];
    const { rerender } = renderHook(
      ({ workspaceId }: { workspaceId: string }) => {
        const store = useWorkspaceStore({ workspaceId, services });
        observed.push({
          requestedWorkspaceId: workspaceId,
          stateWorkspaceId: store.state.workspaceId,
          activeWorktreePath: store.state.activeWorktreePath,
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
        activeWorktreePath: featureWorktree.path,
      })),
    );
  });

  it("retains target workspace tabs and avoids spawning terminals across alpha -> beta -> alpha switch under render retries", async () => {
    const { services } = createServices();
    const alphaTab: TerminalTab = { id: "tab-alpha", label: "alpha", sessionId: "session-alpha" };
    const betaTab: TerminalTab = { id: "tab-beta", label: "beta", sessionId: "session-beta" };

    const alphaState: WorkspaceState = {
      workspaceId: "alpha",
      worktrees: [worktree],
      activeWorktreePath: worktree.path,
      sessions: {
        "session-alpha": restoredSession("session-alpha", "restored-backend-alpha", worktree.path),
      },
      layout: createLayoutState([alphaTab], alphaTab.id),
      worktreeLayouts: {},
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    const betaState: WorkspaceState = {
      workspaceId: "beta",
      worktrees: [featureWorktree],
      activeWorktreePath: featureWorktree.path,
      sessions: {
        "session-beta": restoredSession("session-beta", "restored-backend-beta", featureWorktree.path),
      },
      layout: createLayoutState([betaTab], betaTab.id),
      worktreeLayouts: {},
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    setWorkspaceSnapshot("alpha", alphaState);
    setWorkspaceSnapshot("beta", betaState);

    (services.spawnTerminal as any).mockClear();

    const { result, rerender } = renderHook(
      ({ workspaceId }: { workspaceId: string }) =>
        useWorkspaceStore({ workspaceId, services }),
      {
        initialProps: { workspaceId: "alpha" },
        wrapper: StrictMode,
      },
    );

    expect(result.current.state.workspaceId).toBe("alpha");
    expect(result.current.state.layout.tabs.map((t) => t.id)).toEqual(["tab-alpha"]);

    rerender({ workspaceId: "beta" });
    expect(result.current.state.workspaceId).toBe("beta");
    expect(result.current.state.layout.tabs.map((t) => t.id)).toEqual(["tab-beta"]);

    rerender({ workspaceId: "alpha" });
    expect(result.current.state.workspaceId).toBe("alpha");
    expect(result.current.state.layout.tabs.map((t) => t.id)).toEqual(["tab-alpha"]);

    await act(async () => {
      await result.current.ensureTabForWorktree(worktree);
    });

    expect(result.current.state.layout.tabs.map((t) => t.id)).toEqual(["tab-alpha"]);
    expect(services.spawnTerminal).not.toHaveBeenCalled();
  });

  it("keeps active worktree tab visible without zero-tab flicker while unpopulated worktree spawn is pending", async () => {
    let resolveDeferredSpawnB: (backendId: string) => void = () => {
      throw new Error("resolveDeferredSpawnB not initialized");
    };
    const deferredSpawnBPromise = new Promise<string>((resolve) => {
      resolveDeferredSpawnB = resolve;
    });

    const { services } = createServices();
    let sessionNumber = 0;
    vi.mocked(services.spawnTerminal).mockImplementation(async (request) => {
      if (request.worktree?.slug === "feature" || request.cwd === featureWorktree.path) {
        return deferredSpawnBPromise;
      }
      return `backend-${++sessionNumber}`;
    });

    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree, featureWorktree], services }),
    );

    let tabA: string | null = null;
    await act(async () => {
      tabA = openedTab(await result.current.openTab(worktree, "worktree-A-tab"));
    });
    if (!tabA) throw new Error("tabA was not created");

    expect(result.current.state.activeWorktreePath).toBe(worktree.path);
    expect(result.current.state.layout.tabs.map((t) => t.id)).toEqual([tabA]);
    expect(result.current.state.layout.tabs).toHaveLength(1);
    expect(services.spawnTerminal).toHaveBeenCalledTimes(1);

    // Trigger switch to unpopulated worktree B while spawn is held unresolved
    let switchPromise: Promise<string | null> | null = null;
    act(() => {
      switchPromise = result.current.ensureTabForWorktree(featureWorktree);
    });
    if (!switchPromise) throw new Error("switchPromise was not created");

    // While spawn is in-flight, A's tab and layout must remain visible and non-empty (no zero-tab intermediate frame)
    expect(result.current.state.activeWorktreePath).toBe(worktree.path);
    expect(result.current.state.layout.tabs).not.toHaveLength(0);
    expect(result.current.state.layout.tabs.map((t) => t.id)).toEqual([tabA]);
    expect(result.current.state.layout.activeTabId).toBe(tabA);

    // Resolve B's spawn
    let tabBId: string | null = null;
    await act(async () => {
      resolveDeferredSpawnB("backend-feature-1");
      tabBId = await switchPromise;
    });

    // Final state assertions: active worktree is B, layout contains B's tab, exactly 1 terminal spawned for B (2 total)
    expect(result.current.state.activeWorktreePath).toBe(featureWorktree.path);
    expect(tabBId).toBeTruthy();
    expect(result.current.state.layout.tabs.map((t) => t.id)).toEqual([tabBId]);
    expect(result.current.state.layout.activeTabId).toBe(tabBId);
    expect(services.spawnTerminal).toHaveBeenCalledTimes(2);
  });

  it("maintains independent tabs and sessions per worktree when switching between worktrees", async () => {
    const { services } = createServices();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree, featureWorktree], services }),
    );

    let tabA1!: string;
    let tabA2!: string;
    let tabB1!: string;

    await act(async () => {
      tabA1 = openedTab(await result.current.openTab(worktree, "main-1"));
      tabA2 = openedTab(await result.current.openTab(worktree, "main-2"));
    });

    expect(result.current.state.activeWorktreePath).toBe(worktree.path);
    expect(result.current.state.layout.tabs.map((t) => t.id)).toEqual([tabA1, tabA2]);

    await act(async () => {
      tabB1 = openedTab(await result.current.openTab(featureWorktree, "feature-1"));
    });

    // Selecting worktree B must display only worktree B's tabs, without worktree A's tabs
    expect(result.current.state.activeWorktreePath).toBe(featureWorktree.path);
    expect(result.current.state.layout.tabs.map((t) => t.id)).toEqual([tabB1]);

    // Switching back to worktree A must restore worktree A's tabs without worktree B's tabs
    await act(async () => {
      await result.current.ensureTabForWorktree(worktree);
    });

    expect(result.current.state.activeWorktreePath).toBe(worktree.path);
    expect(result.current.state.layout.tabs.map((t) => t.id)).toEqual([tabA1, tabA2]);
  });

  it("isolates browser tabs to the worktree in which they were created", async () => {
    (window as any).__TAURI_INTERNALS__ = {
      invoke: vi.fn(async (cmd: string) => {
        if (cmd === "cmd_browser_create") {
          return {
            browserId: "browser-mock-1",
            webviewLabel: "browser-webview-1",
            workspaceId: "ws-main",
            worktreePath: worktree.path,
            profileId: "default",
            generation: 1,
            url: "http://localhost:3000/docs",
            title: "Docs",
            loading: false,
            canGoBack: false,
            canGoForward: false,
            zoomFactor: 1,
            loadError: null,
            visible: true,
          };
        }
        return undefined;
      }),
    };

    const { services } = createServices();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree, featureWorktree], services }),
    );

    let terminalTabA!: string;
    let browserTabA!: string;
    let terminalTabB!: string;

    await act(async () => {
      terminalTabA = openedTab(await result.current.openTab(worktree, "terminal-A"));
      browserTabA = openedTab(await result.current.createBrowserTab("http://localhost:3000/docs", "Docs"));
    });

    expect(result.current.state.activeWorktreePath).toBe(worktree.path);
    expect(result.current.state.layout.tabs.map((t) => t.id)).toEqual([terminalTabA, browserTabA]);

    await act(async () => {
      terminalTabB = openedTab(await result.current.openTab(featureWorktree, "terminal-B"));
    });

    // Worktree B should have its own tab set and not show Worktree A's browser tab
    expect(result.current.state.activeWorktreePath).toBe(featureWorktree.path);
    expect(result.current.state.layout.tabs.map((t) => t.id)).toEqual([terminalTabB]);
  });

  it("navigates across five parked workspaces with 20 live terminal tabs each as a state-only operation", async () => {
    const { services } = createServices();
    const parkedWorktrees: Worktree[] = Array.from({ length: 5 }, (_, i) => ({
      path: `/repo/parked-${i}`,
      head: `commit-${i}`,
      branch: `refs/heads/orca/ws-${i}/feature-${i}`,
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    }));

    const allWorktrees = [worktree, ...parkedWorktrees];
    const sessions: Record<string, TerminalSession> = {};
    const worktreeLayouts: Record<string, LayoutState> = {};

    for (let w = 0; w < 5; w++) {
      const wt = parkedWorktrees[w];
      const tabs: TerminalTab[] = [];
      for (let t = 0; t < 20; t++) {
        const sessionId = `session-parked-${w}-${t}`;
        const backendSessionId = `backend-parked-${w}-${t}`;
        const tabId = `tab-parked-${w}-${t}`;
        sessions[sessionId] = {
          id: sessionId,
          cwd: wt.path,
          workspaceId: "ws-main",
          worktree: { wsId: `ws-${w}`, slug: `feature-${w}` },
          backendSessionId,
          lifecycle: "working",
        };
        tabs.push({
          id: tabId,
          label: `tab-${w}-${t}`,
          sessionId,
        });
      }
      worktreeLayouts[wt.path] = createLayoutState(tabs, tabs[0].id);
    }

    const primaryTab: TerminalTab = { id: "tab-main", label: "main", sessionId: "session-main" };
    sessions["session-main"] = restoredSession("session-main", "restored-main-backend", worktree.path);

    const initialState: WorkspaceState = {
      worktrees: allWorktrees,
      activeWorktreePath: worktree.path,
      sessions,
      layout: createLayoutState([primaryTab], primaryTab.id),
      worktreeLayouts,
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: allWorktrees, services }),
    );

    act(() => {
      result.current.restoreWorkspace(initialState);
    });

    expect(Object.keys(result.current.state.worktreeLayouts ?? {})).toHaveLength(5);
    for (let w = 0; w < 5; w++) {
      expect(result.current.state.worktreeLayouts![parkedWorktrees[w].path].tabs).toHaveLength(20);
    }
    expect(Object.keys(result.current.state.sessions)).toHaveLength(101);

    (services.spawnTerminal as any).mockClear();
    (services.closeTerminal as any).mockClear();
    (services.waitForTerminalExit as any).mockClear();
    (services.getTerminalCwd as any).mockClear();
    (services.ensureTerminalEvents as any).mockClear();

    // 1. Activate parked workspace 0 via ensureTabForWorktree
    let activeTabId!: string | null;
    await act(async () => {
      activeTabId = await result.current.ensureTabForWorktree(parkedWorktrees[0]);
    });
    expect(activeTabId).toBe("tab-parked-0-0");
    expect(result.current.state.activeWorktreePath).toBe(parkedWorktrees[0].path);
    expect(result.current.state.layout.tabs).toHaveLength(20);
    expect(result.current.state.layout.activeTabId).toBe("tab-parked-0-0");

    // Switch tabs within active workspace 0
    act(() => {
      result.current.activateTab("tab-parked-0-5");
    });
    expect(result.current.state.layout.activeTabId).toBe("tab-parked-0-5");
    act(() => {
      result.current.activateTab("tab-parked-0-19");
    });
    expect(result.current.state.layout.activeTabId).toBe("tab-parked-0-19");

    // 2. Navigate through all remaining parked workspaces (1 through 4)
    for (let w = 1; w < 5; w++) {
      const wt = parkedWorktrees[w];
      await act(async () => {
        activeTabId = await result.current.ensureTabForWorktree(wt);
      });
      expect(activeTabId).toBe(`tab-parked-${w}-0`);
      expect(result.current.state.activeWorktreePath).toBe(wt.path);
      expect(result.current.state.layout.tabs).toHaveLength(20);
      expect(result.current.state.layout.activeTabId).toBe(`tab-parked-${w}-0`);

      // Activate an internal tab in each workspace
      const targetTabId = `tab-parked-${w}-${(w * 3) % 20}`;
      act(() => {
        result.current.activateTab(targetTabId);
      });
      expect(result.current.state.layout.activeTabId).toBe(targetTabId);
    }

    // 3. Jump directly to a tab in parked workspace 2 via activateTab cross-workspace activation
    act(() => {
      result.current.activateTab("tab-parked-2-14");
    });
    expect(result.current.state.activeWorktreePath).toBe(parkedWorktrees[2].path);
    expect(result.current.state.layout.activeTabId).toBe("tab-parked-2-14");
    expect(result.current.state.layout.tabs).toHaveLength(20);

    // 4. Return to parked workspace 0 via ensureTabForWorktree and verify remembered active tab
    await act(async () => {
      activeTabId = await result.current.ensureTabForWorktree(parkedWorktrees[0]);
    });
    expect(activeTabId).toBe("tab-parked-0-19");
    expect(result.current.state.activeWorktreePath).toBe(parkedWorktrees[0].path);
    expect(result.current.state.layout.activeTabId).toBe("tab-parked-0-19");
    expect(result.current.state.layout.tabs).toHaveLength(20);

    // Explicit assertions verifying zero side-effects and pure state transitions
    expect(services.spawnTerminal).not.toHaveBeenCalled();
    expect(services.closeTerminal).not.toHaveBeenCalled();
    expect(services.waitForTerminalExit).not.toHaveBeenCalled();
    expect(services.ensureTerminalEvents).not.toHaveBeenCalled();
    expect(services.getTerminalCwd).not.toHaveBeenCalled();

    // Verify all 101 live sessions remain intact with valid backend metadata
    expect(Object.keys(result.current.state.sessions)).toHaveLength(101);
    for (let w = 0; w < 5; w++) {
      for (let t = 0; t < 20; t++) {
        const session = result.current.state.sessions[`session-parked-${w}-${t}`];
        expect(session).toBeDefined();
        expect(session.backendSessionId).toBe(`backend-parked-${w}-${t}`);
        expect(session.cwd).toBe(parkedWorktrees[w].path);
      }
    }
  });

  it("closes browser when closing the sole browser tab", async () => {
    const closeBrowserSpy = vi.spyOn(browserTauri, "closeBrowser").mockResolvedValue(undefined);
    const { services } = createServices();
    const { result } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree], services }),
    );

    const singleBrowserTab: BrowserTab = {
      kind: "browser",
      id: "tab-browser-1",
      label: "Docs",
      browserId: "browser-123",
      url: "http://localhost:3000/docs",
      worktreePath: worktree.path,
    };

    const singleBrowserState: WorkspaceState = {
      worktrees: [worktree],
      activeWorktreePath: worktree.path,
      sessions: {},
      layout: {
        tabs: [singleBrowserTab],
        activeTabId: singleBrowserTab.id,
        layoutsByTabId: {
          [singleBrowserTab.id]: {
            root: { type: "leaf", leafId: "leaf-browser" },
            activeLeafId: "leaf-browser",
            expandedLeafId: null,
            sessionIdsByLeafId: {},
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    act(() => {
      result.current.restoreWorkspace(singleBrowserState);
    });

    expect(result.current.state.layout.tabs).toHaveLength(1);
    expect(result.current.state.layout.tabs[0].kind).toBe("browser");

    await act(async () => {
      await result.current.closeTab("tab-browser-1");
    });

    expect(closeBrowserSpy).toHaveBeenCalledWith("browser-123");
    expect(services.spawnTerminal).not.toHaveBeenCalled();
    expect(result.current.state.layout.tabs).toHaveLength(0);
    expect(result.current.state.layout.activeTabId).toBeNull();
    expect(Object.keys(result.current.state.sessions)).toHaveLength(0);
    closeBrowserSpy.mockRestore();
  });

  describe("REBIND_SESSION_BACKEND reducer and ensureSessionBackends", () => {
    it("updates only the target session with backendSessionId and running lifecycle, missing sessionId is a no-op", () => {
      const initialTerminalState: WorkspaceState = {
        worktrees: [worktree],
        activeWorktreePath: worktree.path,
        sessions: {
          "session-target": {
            id: "session-target",
            cwd: "/repo/main/sub",
            worktreePath: worktree.path,
            workspaceId: "ws-main",
            worktree: { wsId: "ws-main", slug: "main" },
            backendSessionId: null,
            lifecycle: "exited",
          },
          "session-other": {
            id: "session-other",
            cwd: worktree.path,
            worktreePath: worktree.path,
            workspaceId: "ws-main",
            worktree: { wsId: "ws-main", slug: "main" },
            backendSessionId: "backend-existing",
            lifecycle: "working",
          },
        },
        layout: createLayoutState([
          { kind: "terminal", id: "tab-1", label: "Tab 1", sessionId: "session-target" },
          { kind: "terminal", id: "tab-2", label: "Tab 2", sessionId: "session-other" },
        ]),
        unreadTabIds: {},
        unreadWorktreePaths: {},
      };

      const updatedState = workspaceReducer(initialTerminalState, {
        type: "REBIND_SESSION_BACKEND",
        sessionId: "session-target",
        backendSessionId: "backend-new-123",
      } as any);

      expect(updatedState.sessions["session-target"]).toEqual({
        ...initialTerminalState.sessions["session-target"],
        backendSessionId: "backend-new-123",
        lifecycle: "running",
      });
      expect(updatedState.sessions["session-other"]).toBe(initialTerminalState.sessions["session-other"]);

      const noOpState = workspaceReducer(initialTerminalState, {
        type: "REBIND_SESSION_BACKEND",
        sessionId: "non-existent-session",
        backendSessionId: "backend-new-456",
      } as any);
      expect(noOpState).toBe(initialTerminalState);
    });

    it("ensureSessionBackends calls spawnTerminal exactly once for sessions with null backendSessionId and rebinds them", async () => {
      const { services } = createServices();
      const { result } = renderHook(() =>
        useWorkspaceStore({ initialWorktrees: [worktree], services }),
      );

      const stateWithDeadSessions: WorkspaceState = {
        worktrees: [worktree],
        activeWorktreePath: worktree.path,
        sessions: {
          "session-1": {
            id: "session-1",
            cwd: "/repo/main/nested",
            worktreePath: worktree.path,
            workspaceId: "ws-main",
            worktree: { wsId: "ws-main", slug: "main" },
            backendSessionId: null,
            lifecycle: "exited",
          },
          "session-2": {
            id: "session-2",
            cwd: "/repo/main",
            worktreePath: worktree.path,
            workspaceId: "ws-main",
            worktree: null,
            backendSessionId: null,
            lifecycle: "exited",
          },
          "session-live": {
            id: "session-live",
            cwd: worktree.path,
            worktreePath: worktree.path,
            workspaceId: "ws-main",
            worktree: { wsId: "ws-main", slug: "main" },
            backendSessionId: "backend-already-live",
            lifecycle: "working",
          },
        },
        layout: createLayoutState([
          { kind: "terminal", id: "tab-1", label: "Tab 1", sessionId: "session-1" },
          { kind: "terminal", id: "tab-2", label: "Tab 2", sessionId: "session-2" },
          { kind: "terminal", id: "tab-3", label: "Tab 3", sessionId: "session-live" },
        ]),
        unreadTabIds: {},
        unreadWorktreePaths: {},
      };

      act(() => {
        result.current.restoreWorkspace(stateWithDeadSessions);
      });

      await act(async () => {
        await (result.current as any).ensureSessionBackends(["session-1", "session-2", "session-live"]);
      });

      expect(services.spawnTerminal).toHaveBeenCalledTimes(2);
      expect(services.spawnTerminal).toHaveBeenCalledWith({
        workspaceId: expect.any(String),
        worktree: { wsId: "ws-main", slug: "main" },
        cwd: "/repo/main/nested",
      });
      expect(services.spawnTerminal).toHaveBeenCalledWith({
        workspaceId: expect.any(String),
        worktree: null,
        cwd: "/repo/main",
      });

      expect(result.current.state.sessions["session-1"].backendSessionId).toBe("backend-1");
      expect(result.current.state.sessions["session-1"].lifecycle).toBe("running");
      expect(result.current.state.sessions["session-2"].backendSessionId).toBe("backend-2");
      expect(result.current.state.sessions["session-2"].lifecycle).toBe("running");
      expect(result.current.state.sessions["session-live"].backendSessionId).toBe("backend-already-live");
      expect(result.current.state.sessions["session-live"].lifecycle).toBe("working");

      // Does not spawn again for already bound sessions
      (services.spawnTerminal as any).mockClear();
      await act(async () => {
        await (result.current as any).ensureSessionBackends(["session-1", "session-2", "session-live"]);
      });
      expect(services.spawnTerminal).not.toHaveBeenCalled();
    });

    it("guards against concurrent double-spawns and retries on spawn failure after clearing in-flight guard", async () => {
      let deferredSpawn!: { resolve: (id: string) => void; reject: (err: Error) => void; promise: Promise<string> };
      const resetDeferred = () => {
        let resolve!: (id: string) => void;
        let reject!: (err: Error) => void;
        const promise = new Promise<string>((res, rej) => {
          resolve = res;
          reject = rej;
        });
        deferredSpawn = { resolve, reject, promise };
      };
      resetDeferred();

      const { services } = createServices();
      services.spawnTerminal = vi.fn(() => deferredSpawn.promise);

      const { result } = renderHook(() =>
        useWorkspaceStore({ initialWorktrees: [worktree], services }),
      );

      const state: WorkspaceState = {
        worktrees: [worktree],
        activeWorktreePath: worktree.path,
        sessions: {
          "session-inflight": {
            id: "session-inflight",
            cwd: worktree.path,
            worktreePath: worktree.path,
            workspaceId: "ws-main",
            worktree: null,
            backendSessionId: null,
            lifecycle: "exited",
          },
        },
        layout: createLayoutState([{ kind: "terminal", id: "tab-1", label: "Tab 1", sessionId: "session-inflight" }]),
        unreadTabIds: {},
        unreadWorktreePaths: {},
      };

      act(() => {
        result.current.restoreWorkspace(state);
      });

      // 1. Concurrent calls while spawn is in flight
      let call1!: Promise<void>;
      let call2!: Promise<void>;
      act(() => {
        call1 = (result.current as any).ensureSessionBackends(["session-inflight"]);
        call2 = (result.current as any).ensureSessionBackends(["session-inflight"]);
      });
      await act(async () => {
        await Promise.resolve();
      });

      expect(services.spawnTerminal).toHaveBeenCalledTimes(1);

      // 2. Reject the in-flight spawn to test error swallowing and clearing in-flight entry
      await act(async () => {
        deferredSpawn.reject(new Error("PTY spawn failed"));
        await expect(Promise.all([call1, call2])).resolves.toBeDefined();
      });

      expect(result.current.state.sessions["session-inflight"].backendSessionId).toBeNull();

      // 3. Retry after failure - should attempt spawn again
      resetDeferred();
      let retryCall!: Promise<void>;
      act(() => {
        retryCall = (result.current as any).ensureSessionBackends(["session-inflight"]);
      });
      await act(async () => {
        await Promise.resolve();
      });
      expect(services.spawnTerminal).toHaveBeenCalledTimes(2);

      await act(async () => {
        deferredSpawn.resolve("backend-success-after-retry");
        await retryCall;
      });

      expect(result.current.state.sessions["session-inflight"].backendSessionId).toBe("backend-success-after-retry");
      expect(result.current.state.sessions["session-inflight"].lifecycle).toBe("running");
    });
  });
});
