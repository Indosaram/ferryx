import { useCallback, useEffect, useReducer, useRef } from "react";

import { activityStateToAgentState, summarizeActivities, type ActivitySummary, type TerminalActivity } from "../lib/activity";
import { classifyTerminalTitleActivity, formatTabLabelFromTitle, normalizeTerminalTitle, parseAgentTitle } from "../lib/agentTitle";
import { closeBrowser, createBrowser, navigateBrowser, reloadBrowser } from "../lib/browserTauri";
import { closeTerminal, DEFAULT_WORKSPACE_ID, getTerminalCwd, spawnTerminal, waitForTerminalExit } from "../lib/tauri";
import { ensureTerminalEvents, terminalEventBus } from "../lib/terminalEvents";
import { terminalHostManager } from "../lib/terminalHostManager";
import { worktreeIdentity } from "../lib/types";
import type {
  ActiveAgent,
  BrowserTab,
  LayoutState,
  TerminalLifecycle,
  TerminalLifecyclePayload,
  TerminalSession,
  TerminalTab,
  WorkspaceTab,
  Worktree,
  WorktreeIdentity,
} from "../lib/types";
import { createLayoutState, getGroupForTab, getTabsForGroup, layoutReducer } from "./layout";
import { collectLeafIds, type PaneDirection } from "./paneTree";

const LAST_TAB_EXIT_TIMEOUT_MS = 5_000;

export type WorkspaceState = {
  worktrees: Worktree[];
  activeWorktreePath: string | null;
  sessions: Record<string, TerminalSession>;
  layout: LayoutState;
  unreadTabIds: Record<string, boolean>;
  unreadWorktreePaths: Record<string, boolean>;
  /** Optional for backwards compatibility with persisted/test states created before activity tracking. */
  activityBySessionId?: Record<string, TerminalActivity>;
};

export type WorkspaceServices = {
  ensureTerminalEvents: () => Promise<void>;
  spawnTerminal: (request: { workspaceId: string; worktree: WorktreeIdentity | null; cwd?: string | null }) => Promise<string>;
  getTerminalCwd: (sessionId: string) => Promise<string | null>;
  closeTerminal: (sessionId: string) => Promise<void>;
  waitForTerminalExit: (sessionId: string, timeoutMs: number) => Promise<void>;
};

export type SplitPaneOptions = {
  position?: "first" | "second";
};

export type TabSplitEdge = "left" | "right" | "top" | "bottom";

type WorkspaceAction =
  | { type: "SET_WORKTREES"; worktrees: Worktree[] }
  | { type: "RESTORE_WORKSPACE"; state: WorkspaceState }
  | { type: "SELECT_WORKTREE"; path: string }
  | { type: "ADD_TAB_WITH_SESSION"; tab: WorkspaceTab; session?: TerminalSession }
  | {
      type: "CLOSE_TAB";
      tabId: string;
      replacement?: { tab: WorkspaceTab; session?: TerminalSession };
    }
  | { type: "UPDATE_BROWSER_TAB"; tabId: string; updates: Partial<BrowserTab> }
  | { type: "ACTIVATE_TAB"; tabId: string }
  | { type: "REORDER_TAB"; tabId: string; targetIndex: number }
  | { type: "RENAME_TAB"; tabId: string; label: string }
  | { type: "SET_TAB_PINNED"; tabId: string; pinned: boolean }
  | {
      type: "SPLIT_PANE";
      tabId: string;
      targetLeafId?: string;
      direction: PaneDirection;
      position?: "first" | "second";
      newLeafId: string;
      session: TerminalSession;
    }
  | { type: "MOVE_TAB_TO_GROUP"; sourceTabId: string; targetGroupId: string; targetIndex?: number }
  | {
      type: "MOVE_TAB_TO_SPLIT";
      sourceTabId: string;
      targetGroupId: string;
      direction: PaneDirection;
      position?: "first" | "second";
    }
  | {
      type: "DETACH_PANE_TO_TAB";
      sourceTabId: string;
      leafId: string;
      newTab: TerminalTab;
      targetGroupId?: string;
      targetIndex?: number;
    }
  | {
      type: "CLOSE_PANE";
      tabId: string;
      leafId: string;
    }
  | { type: "FOCUS_PANE"; tabId: string; leafId: string }
  | { type: "SET_PANE_RATIO"; tabId: string; path: string; ratio: number }
  | { type: "SET_TAB_GROUP_RATIO"; path: string; ratio: number }
  | { type: "SWAP_PANES"; tabId: string; sourceLeafId: string; targetLeafId: string }
  | { type: "SESSION_LIFECYCLE"; backendSessionId: string; lifecycle: TerminalLifecycle }
  | { type: "SESSION_TITLE_ACTIVITY"; tabId: string; sessionId: string; title: string }
  | { type: "MARK_TAB_UNREAD"; tabId: string }
  | { type: "CLEAR_TAB_UNREAD"; tabId: string }
  | { type: "MARK_WORKTREE_UNREAD"; worktreePath: string }
  | { type: "CLEAR_WORKTREE_UNREAD"; worktreePath: string };

type UseWorkspaceStoreOptions = {
  workspaceId?: string;
  initialWorktrees?: Worktree[];
  services?: WorkspaceServices;
};

const defaultServices: WorkspaceServices = {
  ensureTerminalEvents,
  spawnTerminal,
  getTerminalCwd,
  closeTerminal,
  waitForTerminalExit,
};

export function useWorkspaceStore({
  workspaceId = DEFAULT_WORKSPACE_ID,
  initialWorktrees = [],
  services = defaultServices,
}: UseWorkspaceStoreOptions = {}) {
  const [state, reactDispatch] = useReducer(workspaceReducer, initialWorktrees, createInitialState);
  const stateRef = useRef(state);
  stateRef.current = state;

  const dispatch = useCallback((action: WorkspaceAction) => {
    stateRef.current = workspaceReducer(stateRef.current, action);
    reactDispatch(action);
  }, []);

  useEffect(() => {
    const unsubscribeLifecycle = terminalEventBus.subscribeLifecycle((payload) => {
      dispatch({
        type: "SESSION_LIFECYCLE",
        backendSessionId: payload.sessionId,
        lifecycle: mapBackendLifecycle(payload),
      });
    });
    const unsubscribeTitle = terminalEventBus.subscribeTitle((backendSessionId, title) => {
      const snapshot = stateRef.current;
      const session = Object.values(snapshot.sessions).find(
        (candidate) => candidate.backendSessionId === backendSessionId,
      );
      if (!session) return;
      const tabId = findTabIdForSession(snapshot, session.id);
      if (!tabId) return;
      dispatch({ type: "SESSION_TITLE_ACTIVITY", tabId, sessionId: session.id, title });
    });

    return () => {
      unsubscribeLifecycle();
      unsubscribeTitle();
    };
  }, [dispatch]);

  const createSpawnedTab = useCallback(
    async (worktree: Worktree, label?: string) => {
      await services.ensureTerminalEvents();
      const backendSessionId = await services.spawnTerminal({
        workspaceId,
        worktree: worktreeIdentity(worktree),
        cwd: worktree.path,
      });
      const sessionId = createId("session");
      const tabId = createId("tab");
      const session: TerminalSession = {
        id: sessionId,
        cwd: worktree.path,
        worktreePath: worktree.path,
        workspaceId,
        worktree: worktreeIdentity(worktree),
        backendSessionId,
        lifecycle: "working",
      };
      const tab: TerminalTab = {
        id: tabId,
        label: label ?? nextTabLabel(worktree, stateRef.current.layout.tabs, stateRef.current.sessions),
        sessionId,
      };
      return { tab, session };
    },
    [services, workspaceId],
  );

  const openTab = useCallback(
    async (worktree: Worktree) => {
      const binding = await createSpawnedTab(worktree);
      dispatch({ type: "ADD_TAB_WITH_SESSION", ...binding });
      if (!stateRef.current.worktrees.some((candidate) => candidate.path === worktree.path)) {
        dispatch({ type: "SET_WORKTREES", worktrees: [...stateRef.current.worktrees, worktree] });
      }
      dispatch({ type: "SELECT_WORKTREE", path: worktree.path });
      return binding.tab.id;
    },
    [createSpawnedTab, dispatch],
  );

  const ensureTabForWorktree = useCallback(
    async (worktree: Worktree) => {
      const snapshot = stateRef.current;
      const existing = snapshot.layout.tabs.find(
        (tab) => tab.kind !== "browser" && sessionWorktreePath(snapshot.sessions[tab.sessionId]) === worktree.path,
      );
      dispatch({ type: "SELECT_WORKTREE", path: worktree.path });
      if (existing) {
        dispatch({ type: "ACTIVATE_TAB", tabId: existing.id });
        return existing.id;
      }
      return openTab(worktree);
    },
    [dispatch, openTab],
  );

  const splitPane = useCallback(
    async (
      tabId: string,
      targetLeafId: string,
      direction: PaneDirection,
      options: SplitPaneOptions = {},
    ) => {
      const snapshot = stateRef.current;
      const targetTab = snapshot.layout.tabs.find((candidate) => candidate.id === tabId);
      if (!targetTab || targetTab.kind === "browser") return;
      const targetLayout = snapshot.layout.layoutsByTabId?.[targetTab.id];
      if (!targetLayout || !collectLeafIds(targetLayout.root).includes(targetLeafId)) return;

      const sourceLocalSessionId = targetLayout.sessionIdsByLeafId[targetLeafId] ?? targetTab.sessionId;
      const sourceSession = snapshot.sessions[sourceLocalSessionId];
      if (!sourceSession) return;

      await services.ensureTerminalEvents();
      let inheritedCwd = sourceSession.cwd;
      if (sourceSession.backendSessionId) {
        try {
          inheritedCwd = (await services.getTerminalCwd(sourceSession.backendSessionId)) ?? sourceSession.cwd;
        } catch {
          inheritedCwd = sourceSession.cwd;
        }
      }

      const backendSessionId = await services.spawnTerminal({
        workspaceId,
        worktree: sourceSession.worktree,
        cwd: inheritedCwd,
      });
      const localSessionId = createId("session");
      const newLeafId = createId("leaf");
      const session: TerminalSession = {
        id: localSessionId,
        cwd: inheritedCwd,
        worktreePath: sessionWorktreePath(sourceSession),
        workspaceId: sourceSession.workspaceId || workspaceId,
        worktree: sourceSession.worktree,
        backendSessionId,
        lifecycle: "working",
      };

      dispatch({
        type: "SPLIT_PANE",
        tabId: targetTab.id,
        targetLeafId,
        direction,
        position: options.position,
        newLeafId,
        session,
      });

      if (!stateRef.current.sessions[localSessionId]) {
        terminalEventBus.clearSession(backendSessionId);
        await services.closeTerminal(backendSessionId).catch(() => undefined);
      }
    },
    [dispatch, services, workspaceId],
  );

  const moveTabToGroup = useCallback(
    (sourceTabId: string, targetGroupId: string, targetIndex?: number) => {
      dispatch({ type: "MOVE_TAB_TO_GROUP", sourceTabId, targetGroupId, targetIndex });
    },
    [dispatch],
  );

  const moveTabToSplit = useCallback(
    (sourceTabId: string, targetGroupId: string, direction: PaneDirection, position: "first" | "second" = "second") => {
      dispatch({ type: "MOVE_TAB_TO_SPLIT", sourceTabId, targetGroupId, direction, position });
    },
    [dispatch],
  );

  const detachPaneToTab = useCallback(
    (sourceTabId: string, leafId: string, targetGroupId?: string, targetIndex?: number) => {
      const snapshot = stateRef.current;
      const sourceTab = snapshot.layout.tabs.find((tab) => tab.id === sourceTabId);
      const sourceLayout = snapshot.layout.layoutsByTabId[sourceTabId];
      if (!sourceTab || sourceTab.kind === "browser" || !sourceLayout || collectLeafIds(sourceLayout.root).length <= 1) return null;
      const sessionId = sourceLayout.sessionIdsByLeafId[leafId];
      if (!sessionId || !snapshot.sessions[sessionId]) return null;
      const newTabId = createId("tab");
      const newTab: TerminalTab = {
        id: newTabId,
        label: sourceTab.label,
        sessionId,
      };
      dispatch({ type: "DETACH_PANE_TO_TAB", sourceTabId, leafId, newTab, targetGroupId, targetIndex });
      return stateRef.current.layout.tabs.some((tab) => tab.id === newTabId) ? newTabId : null;
    },
    [dispatch],
  );

  const closeTab = useCallback(
    async (tabId: string) => {
      const snapshot = stateRef.current;
      const closingTab = snapshot.layout.tabs.find((tab) => tab.id === tabId);
      if (!closingTab || closingTab.pinned) return;

      if (closingTab.kind === "browser") {
        await closeBrowser(closingTab.browserId);
        dispatch({ type: "CLOSE_TAB", tabId });
        return;
      }

      const closingSession = snapshot.sessions[closingTab.sessionId];
      const disposableSessions = getDisposableSessionsForTab(snapshot, tabId);
      const tabSessionIds = getTabSessionIds(snapshot, tabId);

      if (snapshot.layout.tabs.length === 1) {
        const closingWorktreePath = sessionWorktreePath(closingSession);
        const worktree =
          (closingWorktreePath && snapshot.worktrees.find((candidate) => candidate.path === closingWorktreePath)) ??
          getActiveWorktree(snapshot);
        if (!worktree) return;

        await Promise.all(
          disposableSessions.map((session) => closeBackendSessionAndWait(session, services)),
        );
        for (const sessionId of tabSessionIds) {
          if (!isSessionReferencedOutsideTab(snapshot, sessionId, tabId)) {
            terminalHostManager.destroy(sessionId);
          }
        }
        const replacement = await createSpawnedTab(worktree, closingTab.label);
        dispatch({ type: "CLOSE_TAB", tabId, replacement });
        return;
      }

      dispatch({ type: "CLOSE_TAB", tabId });
      for (const sessionId of tabSessionIds) {
        if (!isSessionReferencedOutsideTab(snapshot, sessionId, tabId)) {
          terminalHostManager.destroy(sessionId);
        }
      }
      await Promise.allSettled(disposableSessions.map((session) => closeBackendSession(session, services)));
    },
    [createSpawnedTab, dispatch, services],
  );

  const closePane = useCallback(
    async (tabId: string, leafId: string) => {
      const snapshot = stateRef.current;
      const tabLayout = snapshot.layout.layoutsByTabId?.[tabId];
      if (!tabLayout) return;
      if (tabLayout.root.type === "leaf") {
        if (tabLayout.root.leafId === leafId) await closeTab(tabId);
        return;
      }

      const closingSessionId = tabLayout.sessionIdsByLeafId[leafId];
      const closingSession = closingSessionId ? snapshot.sessions[closingSessionId] : undefined;
      dispatch({ type: "CLOSE_PANE", tabId, leafId });
      if (closingSessionId && !isSessionReferenced(stateRef.current, closingSessionId)) {
        terminalHostManager.destroy(closingSessionId);
        await closeBackendSession(closingSession, services);
      }
    },
    [closeTab, dispatch, services],
  );

  const closeTabs = useCallback(
    async (tabIds: string[]) => {
      for (const tabId of tabIds) await closeTab(tabId);
    },
    [closeTab],
  );

  const closeOtherTabs = useCallback(
    async (tabId: string) => {
      const snapshot = stateRef.current;
      const group = getGroupForTab(snapshot.layout, tabId);
      const tabs = group ? getTabsForGroup(snapshot.layout, group.id) : [];
      await closeTabs(tabs.filter((tab) => tab.id !== tabId && !tab.pinned).map((tab) => tab.id));
    },
    [closeTabs],
  );

  const closeTabsToRight = useCallback(
    async (tabId: string) => {
      const snapshot = stateRef.current;
      const group = getGroupForTab(snapshot.layout, tabId);
      const tabs = group ? getTabsForGroup(snapshot.layout, group.id) : [];
      const index = tabs.findIndex((tab) => tab.id === tabId);
      if (index < 0) return;
      await closeTabs(tabs.slice(index + 1).filter((tab) => !tab.pinned).map((tab) => tab.id));
    },
    [closeTabs],
  );

  const closeTabsToLeft = useCallback(
    async (tabId: string) => {
      const snapshot = stateRef.current;
      const group = getGroupForTab(snapshot.layout, tabId);
      const tabs = group ? getTabsForGroup(snapshot.layout, group.id) : [];
      const index = tabs.findIndex((tab) => tab.id === tabId);
      if (index < 0) return;
      await closeTabs(tabs.slice(0, index).filter((tab) => !tab.pinned).map((tab) => tab.id));
    },
    [closeTabs],
  );

  const syncWorktrees = useCallback(
    async (worktrees: Worktree[]) => {
      const snapshot = stateRef.current;
      const validWorktreePaths = new Set(worktrees.map((worktree) => worktree.path));
      const staleSessions = Object.values(snapshot.sessions).filter((session) => !validWorktreePaths.has(sessionWorktreePath(session)));
      dispatch({ type: "SET_WORKTREES", worktrees });
      for (const session of staleSessions) {
        terminalHostManager.destroy(session.id);
      }
      await Promise.allSettled(staleSessions.map((session) => closeBackendSession(session, services)));
    },
    [dispatch, services],
  );

  const activateTab = useCallback((tabId: string) => dispatch({ type: "ACTIVATE_TAB", tabId }), [dispatch]);
  const reorderTab = useCallback(
    (tabId: string, targetIndex: number) => dispatch({ type: "REORDER_TAB", tabId, targetIndex }),
    [dispatch],
  );
  const renameTab = useCallback(
    (tabId: string, label: string) => dispatch({ type: "RENAME_TAB", tabId, label }),
    [dispatch],
  );
  const setTabPinned = useCallback(
    (tabId: string, pinned: boolean) => dispatch({ type: "SET_TAB_PINNED", tabId, pinned }),
    [dispatch],
  );
  const focusPane = useCallback((tabId: string, leafId: string) => dispatch({ type: "FOCUS_PANE", tabId, leafId }), [dispatch]);
  const setPaneRatio = useCallback(
    (tabId: string, path: string, ratio: number) => dispatch({ type: "SET_PANE_RATIO", tabId, path, ratio }),
    [dispatch],
  );
  const setTabGroupRatio = useCallback(
    (path: string, ratio: number) => dispatch({ type: "SET_TAB_GROUP_RATIO", path, ratio }),
    [dispatch],
  );
  const swapPanes = useCallback(
    (tabId: string, sourceLeafId: string, targetLeafId: string) => dispatch({ type: "SWAP_PANES", tabId, sourceLeafId, targetLeafId }),
    [dispatch],
  );

  const restoreWorkspace = useCallback(
    (restoredState: WorkspaceState) => dispatch({ type: "RESTORE_WORKSPACE", state: restoredState }),
    [dispatch],
  );

  const updateSessionTitleActivity = useCallback(
    (tabId: string, title: string, explicitSessionId?: string) => {
      const snapshot = stateRef.current;
      const tab = snapshot.layout.tabs.find((candidate) => candidate.id === tabId);
      if (!tab || tab.kind === "browser") return;
      const sessionId = explicitSessionId ?? tab.sessionId;
      if (!snapshot.sessions[sessionId]) return;
      dispatch({ type: "SESSION_TITLE_ACTIVITY", tabId, sessionId, title });
    },
    [dispatch],
  );

  const createBrowserTab = useCallback(
    async (url = "http://localhost:3000", label?: string) => {
      const activeWorktree = getActiveWorktree(stateRef.current);
      const browserState = await createBrowser({
        workspaceId,
        worktreePath: activeWorktree?.path,
        url,
        visible: true,
      });

      const tabId = createId("tab");
      const browserTab: BrowserTab = {
        kind: "browser",
        id: tabId,
        label: label ?? "Browser",
        browserId: browserState.browserId,
        url: browserState.url,
        title: browserState.title,
        loading: browserState.loading,
        canGoBack: browserState.canGoBack,
        canGoForward: browserState.canGoForward,
      };

      dispatch({ type: "ADD_TAB_WITH_SESSION", tab: browserTab });
      return tabId;
    },
    [dispatch, workspaceId],
  );

  const navigateBrowserTabAction = useCallback(
    async (tabId: string, url: string) => {
      const tab = stateRef.current.layout.tabs.find((candidate) => candidate.id === tabId);
      if (!tab || tab.kind !== "browser") return;
      dispatch({ type: "UPDATE_BROWSER_TAB", tabId, updates: { url, loading: true } });
      try {
        await navigateBrowser(tab.browserId, url);
      } finally {
        if (stateRef.current.layout.tabs.some((candidate) => candidate.id === tabId)) {
          dispatch({ type: "UPDATE_BROWSER_TAB", tabId, updates: { loading: false } });
        }
      }
    },
    [dispatch],
  );

  const reloadBrowserTabAction = useCallback(
    async (tabId: string) => {
      const tab = stateRef.current.layout.tabs.find((candidate) => candidate.id === tabId);
      if (!tab || tab.kind !== "browser") return;
      dispatch({ type: "UPDATE_BROWSER_TAB", tabId, updates: { loading: true } });
      try {
        await reloadBrowser(tab.browserId);
      } finally {
        if (stateRef.current.layout.tabs.some((candidate) => candidate.id === tabId)) {
          dispatch({ type: "UPDATE_BROWSER_TAB", tabId, updates: { loading: false } });
        }
      }
    },
    [dispatch],
  );

  const openWorkspacePortInBrowser = useCallback(
    async (port: number) => {
      const url = `http://localhost:${port}`;
      return createBrowserTab(url, `Port ${port}`);
    },
    [createBrowserTab],
  );

  return {
    state,
    agents: selectAgents(state),
    tabActivity: selectTabActivitySummaries(state),
    worktreeActivity: selectWorktreeActivitySummaries(state),
    openTab,
    createBrowserTab,
    navigateBrowserTab: navigateBrowserTabAction,
    reloadBrowserTab: reloadBrowserTabAction,
    ensureTabForWorktree,
    openWorkspacePortInBrowser,
    closeTab,
    closeOtherTabs,
    closeTabsToRight,
    closeTabsToLeft,
    splitPane,
    moveTabToGroup,
    moveTabToSplit,
    detachPaneToTab,
    closePane,
    activateTab,
    reorderTab,
    renameTab,
    setTabPinned,
    focusPane,
    setPaneRatio,
    setTabGroupRatio,
    swapPanes,
    syncWorktrees,
    restoreWorkspace,
    updateSessionTitleActivity,
    markTabUnread: (tabId: string) => dispatch({ type: "MARK_TAB_UNREAD", tabId }),
    clearTabUnread: (tabId: string) => dispatch({ type: "CLEAR_TAB_UNREAD", tabId }),
    markWorktreeUnread: (worktreePath: string) => dispatch({ type: "MARK_WORKTREE_UNREAD", worktreePath }),
    clearWorktreeUnread: (worktreePath: string) => dispatch({ type: "CLEAR_WORKTREE_UNREAD", worktreePath }),
  };
}

export function selectAgents(state: WorkspaceState): ActiveAgent[] {
  return state.layout.tabs.flatMap((tab) => {
    if (tab.kind === "browser") return [];
    const session = state.sessions[tab.sessionId];
    if (!session) return [];
    const worktreePath = sessionWorktreePath(session);
    const worktree = state.worktrees.find((candidate) => candidate.path === worktreePath);
    const activity = state.activityBySessionId?.[session.id];
    const parsed = activity ? parseAgentTitle(activity.title) : null;
    return [
      {
        id: session.backendSessionId ?? session.id,
        name: parsed?.isAgent ? parsed.name : tab.label,
        task: parsed?.isAgent
          ? parsed.task
          : worktree?.branch?.replace(/^refs\/heads\//, "") ?? worktreePath,
        state: activity ? activityStateToAgentState(activity.state) : session.lifecycle,
        worktree: session.worktree,
        worktreePath,
        sessionId: session.backendSessionId ?? session.id,
      },
    ];
  });
}

export function selectTabActivitySummaries(state: WorkspaceState): Record<string, ActivitySummary> {
  const result: Record<string, ActivitySummary> = {};
  const activityBySessionId = state.activityBySessionId ?? {};

  for (const tab of state.layout.tabs) {
    if (tab.kind === "browser") continue;
    const sessionIds = getTabSessionIds(state, tab.id);
    const activities = [...sessionIds]
      .map((sessionId) => activityBySessionId[sessionId])
      .filter((activity): activity is TerminalActivity => Boolean(activity));
    result[tab.id] = summarizeActivities(activities, Boolean(state.unreadTabIds[tab.id]));
  }

  return result;
}

export function selectWorktreeActivitySummaries(state: WorkspaceState): Record<string, ActivitySummary> {
  const result: Record<string, ActivitySummary> = {};
  const activityBySessionId = state.activityBySessionId ?? {};
  const sessionIdsByWorktree = new Map<string, Set<string>>();
  const unreadTabsByWorktree = new Set<string>();

  for (const tab of state.layout.tabs) {
    if (tab.kind === "browser") continue;
    const sessionIds = getTabSessionIds(state, tab.id);
    const worktreePaths = new Set<string>();
    for (const sessionId of sessionIds) {
      const session = state.sessions[sessionId];
      if (!session) continue;
      const worktreePath = sessionWorktreePath(session);
      worktreePaths.add(worktreePath);
      const bucket = sessionIdsByWorktree.get(worktreePath) ?? new Set<string>();
      bucket.add(sessionId);
      sessionIdsByWorktree.set(worktreePath, bucket);
    }
    if (state.unreadTabIds[tab.id]) {
      for (const path of worktreePaths) unreadTabsByWorktree.add(path);
    }
  }

  for (const worktree of state.worktrees) {
    const sessionIds = sessionIdsByWorktree.get(worktree.path) ?? new Set<string>();
    const activities = [...sessionIds]
      .map((sessionId) => activityBySessionId[sessionId])
      .filter((activity): activity is TerminalActivity => Boolean(activity));
    result[worktree.path] = summarizeActivities(
      activities,
      Boolean(state.unreadWorktreePaths[worktree.path] || unreadTabsByWorktree.has(worktree.path)),
    );
  }

  return result;
}

function createInitialState(worktrees: Worktree[]): WorkspaceState {
  return {
    worktrees,
    activeWorktreePath: worktrees[0]?.path ?? null,
    sessions: {},
    layout: createLayoutState(),
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  };
}

function workspaceReducer(state: WorkspaceState, action: WorkspaceAction): WorkspaceState {
  switch (action.type) {
    case "RESTORE_WORKSPACE":
      return {
        ...action.state,
        unreadTabIds: action.state.unreadTabIds ?? {},
        unreadWorktreePaths: action.state.unreadWorktreePaths ?? {},
        activityBySessionId: action.state.activityBySessionId ?? {},
      };
    case "SET_WORKTREES": {
      const validWorktreePaths = new Set(action.worktrees.map((worktree) => worktree.path));
      const sessions = Object.fromEntries(
        Object.entries(state.sessions).filter(([, session]) => validWorktreePaths.has(sessionWorktreePath(session))),
      );
      const activityBySessionId = Object.fromEntries(
        Object.entries(state.activityBySessionId ?? {}).filter(([sessionId]) => Boolean(sessions[sessionId])),
      );
      let layout = state.layout;
      for (const tab of state.layout.tabs) {
        const hasRetainedSession = [...getTabSessionIds(state, tab.id)].some((sessionId) => Boolean(sessions[sessionId]));
        if (tab.kind !== "browser" && !hasRetainedSession) {
          layout = layoutReducer(layout, { type: "CLOSE_TAB", tabId: tab.id });
        }
      }
      const activeWorktreePath = action.worktrees.some((worktree) => worktree.path === state.activeWorktreePath)
        ? state.activeWorktreePath
        : action.worktrees[0]?.path ?? null;
      const unreadTabIds = Object.fromEntries(
        Object.entries(state.unreadTabIds).filter(([tabId]) => layout.tabs.some((tab) => tab.id === tabId)),
      );
      const unreadWorktreePaths = Object.fromEntries(
        Object.entries(state.unreadWorktreePaths).filter(([path]) => validWorktreePaths.has(path)),
      );
      return {
        ...state,
        worktrees: action.worktrees,
        activeWorktreePath,
        sessions,
        layout,
        activityBySessionId,
        unreadTabIds,
        unreadWorktreePaths,
      };
    }
    case "UPDATE_BROWSER_TAB": {
      const tabs = state.layout.tabs.map((tab) => {
        if (tab.id === action.tabId && tab.kind === "browser") return { ...tab, ...action.updates };
        return tab;
      });
      return { ...state, layout: { ...state.layout, tabs } };
    }
    case "SELECT_WORKTREE": {
      if (!state.worktrees.some((worktree) => worktree.path === action.path)) return state;
      const unreadWorktreePaths = { ...state.unreadWorktreePaths };
      delete unreadWorktreePaths[action.path];
      const unreadTabIds = { ...state.unreadTabIds };
      for (const tab of state.layout.tabs) {
        if (getTabWorktreePath(state, tab.id) === action.path) delete unreadTabIds[tab.id];
      }
      return { ...state, activeWorktreePath: action.path, unreadTabIds, unreadWorktreePaths };
    }
    case "MARK_TAB_UNREAD": {
      if (action.tabId === state.layout.activeTabId) return state;
      const worktreePath = getTabWorktreePath(state, action.tabId);
      return {
        ...state,
        unreadTabIds: { ...state.unreadTabIds, [action.tabId]: true },
        unreadWorktreePaths: worktreePath
          ? { ...state.unreadWorktreePaths, [worktreePath]: true }
          : state.unreadWorktreePaths,
      };
    }
    case "CLEAR_TAB_UNREAD": {
      const unreadTabIds = { ...state.unreadTabIds };
      delete unreadTabIds[action.tabId];
      return clearWorktreeUnreadWhenRead({ ...state, unreadTabIds }, action.tabId);
    }
    case "MARK_WORKTREE_UNREAD":
      return {
        ...state,
        unreadWorktreePaths: { ...state.unreadWorktreePaths, [action.worktreePath]: true },
      };
    case "CLEAR_WORKTREE_UNREAD": {
      const unreadWorktreePaths = { ...state.unreadWorktreePaths };
      delete unreadWorktreePaths[action.worktreePath];
      return { ...state, unreadWorktreePaths };
    }
    case "ADD_TAB_WITH_SESSION": {
      const sessions = action.session ? { ...state.sessions, [action.session.id]: action.session } : state.sessions;
      const sessionId = action.session?.id;
      return {
        ...state,
        sessions,
        layout: layoutReducer(state.layout, { type: "ADD_TAB", tab: action.tab, sessionId }),
      };
    }
    case "REORDER_TAB":
      return { ...state, layout: layoutReducer(state.layout, action) };
    case "RENAME_TAB":
      return { ...state, layout: layoutReducer(state.layout, action) };
    case "SET_TAB_PINNED":
      return { ...state, layout: layoutReducer(state.layout, action) };
    case "SPLIT_PANE": {
      const layout = layoutReducer(state.layout, {
        type: "SPLIT_PANE",
        tabId: action.tabId,
        targetLeafId: action.targetLeafId,
        direction: action.direction,
        position: action.position,
        newLeafId: action.newLeafId,
        sessionId: action.session.id,
      });
      if (layout === state.layout) return state;
      return {
        ...state,
        sessions: { ...state.sessions, [action.session.id]: action.session },
        layout,
      };
    }
    case "MOVE_TAB_TO_GROUP": {
      const layout = layoutReducer(state.layout, action);
      if (layout === state.layout) return state;
      const unreadTabIds = { ...state.unreadTabIds };
      delete unreadTabIds[action.sourceTabId];
      return { ...state, layout, unreadTabIds };
    }
    case "MOVE_TAB_TO_SPLIT": {
      const layout = layoutReducer(state.layout, action);
      if (layout === state.layout) return state;
      const unreadTabIds = { ...state.unreadTabIds };
      delete unreadTabIds[action.sourceTabId];
      return { ...state, layout, unreadTabIds };
    }
    case "DETACH_PANE_TO_TAB": {
      const layout = layoutReducer(state.layout, action);
      if (layout === state.layout) return state;
      return { ...state, layout };
    }
    case "CLOSE_PANE": {
      const tab = state.layout.tabs.find((candidate) => candidate.id === action.tabId);
      const tabLayout = state.layout.layoutsByTabId?.[action.tabId];
      if (!tab || tab.kind === "browser" || !tabLayout) return state;
      if (tabLayout.root.type === "leaf" && tabLayout.root.leafId === action.leafId) {
        return workspaceReducer(state, { type: "CLOSE_TAB", tabId: action.tabId });
      }
      const closingSessionId = tabLayout.sessionIdsByLeafId[action.leafId];
      let layout = layoutReducer(state.layout, {
        type: "CLOSE_PANE",
        tabId: action.tabId,
        leafId: action.leafId,
      });
      if (layout === state.layout) return state;

      if (closingSessionId === tab.sessionId) {
        const remainingLayout = layout.layoutsByTabId[action.tabId];
        const replacementSessionId = Object.values(remainingLayout?.sessionIdsByLeafId ?? {}).find(Boolean);
        if (replacementSessionId && replacementSessionId !== tab.sessionId) {
          layout = {
            ...layout,
            tabs: layout.tabs.map((candidate) =>
              candidate.id === action.tabId && candidate.kind !== "browser"
                ? { ...candidate, sessionId: replacementSessionId }
                : candidate,
            ),
          };
        }
      }

      const candidateState: WorkspaceState = { ...state, layout };
      if (!closingSessionId || isSessionReferenced(candidateState, closingSessionId)) return candidateState;
      const sessions = { ...state.sessions };
      delete sessions[closingSessionId];
      const activityBySessionId = { ...(state.activityBySessionId ?? {}) };
      delete activityBySessionId[closingSessionId];
      return { ...candidateState, sessions, activityBySessionId };
    }
    case "CLOSE_TAB": {
      const closingSessionIds = getTabSessionIds(state, action.tabId);
      const sessions = { ...state.sessions };
      const activityBySessionId = { ...(state.activityBySessionId ?? {}) };
      for (const sessionId of closingSessionIds) {
        if (isSessionReferencedOutsideTab(state, sessionId, action.tabId)) continue;
        delete sessions[sessionId];
        delete activityBySessionId[sessionId];
      }
      if (action.replacement?.session) sessions[action.replacement.session.id] = action.replacement.session;
      const unreadTabIds = { ...state.unreadTabIds };
      delete unreadTabIds[action.tabId];
      const nextState = {
        ...state,
        sessions,
        activityBySessionId,
        unreadTabIds,
        layout: layoutReducer(state.layout, {
          type: "CLOSE_TAB",
          tabId: action.tabId,
          replacementTab: action.replacement?.tab,
        }),
      };
      return clearWorktreeUnreadWhenRead(nextState, action.tabId, state);
    }
    case "ACTIVATE_TAB": {
      if (!state.layout.tabs.some((tab) => tab.id === action.tabId)) return state;
      const unreadTabIds = { ...state.unreadTabIds };
      delete unreadTabIds[action.tabId];
      const nextState = {
        ...state,
        unreadTabIds,
        layout: layoutReducer(state.layout, { type: "ACTIVATE_TAB", tabId: action.tabId }),
      };
      return clearWorktreeUnreadWhenRead(nextState, action.tabId, state);
    }
    case "FOCUS_PANE": {
      const layout = layoutReducer(state.layout, { type: "FOCUS_PANE", tabId: action.tabId, leafId: action.leafId });
      return layout === state.layout ? state : { ...state, layout };
    }
    case "SET_PANE_RATIO": {
      const layout = layoutReducer(state.layout, { type: "SET_PANE_RATIO", tabId: action.tabId, path: action.path, ratio: action.ratio });
      return layout === state.layout ? state : { ...state, layout };
    }
    case "SET_TAB_GROUP_RATIO": {
      const layout = layoutReducer(state.layout, { type: "SET_TAB_GROUP_RATIO", path: action.path, ratio: action.ratio });
      return layout === state.layout ? state : { ...state, layout };
    }
    case "SWAP_PANES": {
      const layout = layoutReducer(state.layout, { type: "SWAP_PANES", tabId: action.tabId, sourceLeafId: action.sourceLeafId, targetLeafId: action.targetLeafId });
      return layout === state.layout ? state : { ...state, layout };
    }
    case "SESSION_LIFECYCLE": {
      const matchedSessionIds: string[] = [];
      const sessions = Object.fromEntries(
        Object.entries(state.sessions).map(([id, session]) => {
          if (session.backendSessionId !== action.backendSessionId) return [id, session];
          matchedSessionIds.push(id);
          return [id, { ...session, lifecycle: action.lifecycle }];
        }),
      ) as Record<string, TerminalSession>;
      let nextState: WorkspaceState = { ...state, sessions };
      if (action.lifecycle === "exited") {
        for (const sessionId of matchedSessionIds) {
          const current = nextState.activityBySessionId?.[sessionId];
          if (!current || current.state === "done") continue;
          const tabId = findTabIdForSession(nextState, sessionId);
          if (!tabId) continue;
          nextState = applySessionActivity(nextState, tabId, sessionId, { ...current, state: "done" });
        }
      }
      return nextState;
    }
    case "SESSION_TITLE_ACTIVITY": {
      const parsed = parseAgentTitle(action.title);
      const classified = classifyTerminalTitleActivity(action.title);

      if (!classified) {
        if (!state.activityBySessionId?.[action.sessionId]) return state;
        const activityBySessionId = { ...(state.activityBySessionId ?? {}) };
        delete activityBySessionId[action.sessionId];
        return { ...state, activityBySessionId };
      }

      const normalizedTitle = normalizeTerminalTitle(action.title);
      const activity: TerminalActivity = {
        state: classified,
        title: formatTabLabelFromTitle(action.title, normalizedTitle),
        isAgent: Boolean(parsed?.isAgent || classified),
        agentType: parsed?.isAgent ? parsed.agentType : undefined,
      };
      return applySessionActivity(state, action.tabId, action.sessionId, activity);
    }
  }
}

function applySessionActivity(
  state: WorkspaceState,
  tabId: string,
  sessionId: string,
  activity: TerminalActivity,
): WorkspaceState {
  const previous = state.activityBySessionId?.[sessionId];
  if (
    previous &&
    previous.state === activity.state &&
    previous.title === activity.title &&
    previous.isAgent === activity.isAgent &&
    previous.agentType === activity.agentType
  ) {
    return state;
  }

  let nextState: WorkspaceState = {
    ...state,
    activityBySessionId: { ...(state.activityBySessionId ?? {}), [sessionId]: activity },
  };

  if (activity.state === "done" && previous?.state !== "done" && tabId !== state.layout.activeTabId) {
    const worktreePath = sessionWorktreePath(state.sessions[sessionId]) || getTabWorktreePath(state, tabId);
    nextState = {
      ...nextState,
      unreadTabIds: { ...state.unreadTabIds, [tabId]: true },
      unreadWorktreePaths: worktreePath
        ? { ...state.unreadWorktreePaths, [worktreePath]: true }
        : state.unreadWorktreePaths,
    };
  }

  return nextState;
}

function getTabSessionIds(state: WorkspaceState, tabId: string): Set<string> {
  const tab = state.layout.tabs.find((candidate) => candidate.id === tabId);
  if (!tab || tab.kind === "browser") return new Set<string>();
  const tabLayout = state.layout.layoutsByTabId?.[tabId];
  const sessionIds = new Set<string>(Object.values(tabLayout?.sessionIdsByLeafId ?? {}));
  sessionIds.add(tab.sessionId);
  sessionIds.delete("");
  return sessionIds;
}

function isSessionReferencedOutsideTab(state: WorkspaceState, sessionId: string, tabId: string): boolean {
  return state.layout.tabs.some((tab) => tab.id !== tabId && getTabSessionIds(state, tab.id).has(sessionId));
}

function isSessionReferenced(state: WorkspaceState, sessionId: string): boolean {
  return state.layout.tabs.some((tab) => getTabSessionIds(state, tab.id).has(sessionId));
}

function getDisposableSessionsForTab(state: WorkspaceState, tabId: string): TerminalSession[] {
  const result: TerminalSession[] = [];
  const seenBackends = new Set<string>();
  for (const sessionId of getTabSessionIds(state, tabId)) {
    if (isSessionReferencedOutsideTab(state, sessionId, tabId)) continue;
    const session = state.sessions[sessionId];
    if (!session) continue;
    const key = session.backendSessionId ?? session.id;
    if (seenBackends.has(key)) continue;
    seenBackends.add(key);
    result.push(session);
  }
  return result;
}

function sessionWorktreePath(session: TerminalSession | undefined): string {
  return session?.worktreePath ?? session?.cwd ?? "";
}

function getTabWorktreePath(state: WorkspaceState, tabId: string): string | null {
  for (const sessionId of getTabSessionIds(state, tabId)) {
    const path = sessionWorktreePath(state.sessions[sessionId]);
    if (path) return path;
  }
  return null;
}

function findTabIdForSession(state: WorkspaceState, sessionId: string): string | null {
  for (const tab of state.layout.tabs) {
    if (tab.kind === "browser") continue;
    if (getTabSessionIds(state, tab.id).has(sessionId)) return tab.id;
  }
  return null;
}

function clearWorktreeUnreadWhenRead(
  state: WorkspaceState,
  tabId: string,
  sourceState: WorkspaceState = state,
): WorkspaceState {
  const worktreePath = getTabWorktreePath(sourceState, tabId);
  if (!worktreePath) return state;
  const hasOtherUnreadTab = sourceState.layout.tabs.some(
    (tab) => tab.id !== tabId && state.unreadTabIds[tab.id] && getTabWorktreePath(sourceState, tab.id) === worktreePath,
  );
  if (hasOtherUnreadTab) return state;
  const unreadWorktreePaths = { ...state.unreadWorktreePaths };
  delete unreadWorktreePaths[worktreePath];
  return { ...state, unreadWorktreePaths };
}

async function closeBackendSession(session: TerminalSession | undefined, services: WorkspaceServices) {
  if (!session?.backendSessionId) return;
  try {
    await services.closeTerminal(session.backendSessionId);
  } finally {
    terminalEventBus.clearSession(session.backendSessionId);
  }
}

async function closeBackendSessionAndWait(session: TerminalSession | undefined, services: WorkspaceServices) {
  if (!session?.backendSessionId) return;
  const backendSessionId = session.backendSessionId;
  const exitPromise = services.waitForTerminalExit(backendSessionId, LAST_TAB_EXIT_TIMEOUT_MS);
  try {
    await services.closeTerminal(backendSessionId);
    await exitPromise;
  } finally {
    terminalEventBus.clearSession(backendSessionId);
  }
}

function getActiveWorktree(state: WorkspaceState) {
  return state.worktrees.find((worktree) => worktree.path === state.activeWorktreePath) ?? state.worktrees[0] ?? null;
}

function nextTabLabel(worktree: Worktree, tabs: WorkspaceTab[], sessions: Record<string, TerminalSession>) {
  const branch = worktree.branch?.replace(/^refs\/heads\//, "") ?? "";
  const parts = branch.split("/");
  const base =
    parts[0] === "orca" && parts.length > 2
      ? parts.slice(2).join("/")
      : branch && branch !== "orca-lite"
        ? branch
        : "main";
  const count = tabs.filter(
    (tab) => tab.kind !== "browser" && sessionWorktreePath(sessions[tab.sessionId]) === worktree.path,
  ).length + 1;
  return count === 1 ? base : `${base} (${count})`;
}

function mapBackendLifecycle(payload: TerminalLifecyclePayload): TerminalLifecycle {
  if (payload.state === "started") return "working";
  if (payload.state === "failed") return "failed";
  return "exited";
}

function createId(prefix: string) {
  const randomPart = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return `${prefix}:${randomPart}`;
}
