import { useCallback, useEffect, useReducer, useRef } from "react";

import { closeTerminal, DEFAULT_WORKSPACE_ID, spawnTerminal, waitForTerminalExit } from "../lib/tauri";
import { createBrowser, navigateBrowser, reloadBrowser } from "../lib/browserTauri";
import { ensureTerminalEvents, terminalEventBus } from "../lib/terminalEvents";
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
import type { PaneDirection } from "./paneTree";
import { createLayoutState, layoutReducer } from "./layout";

const LAST_TAB_EXIT_TIMEOUT_MS = 5_000;

export type WorkspaceState = {
  worktrees: Worktree[];
  activeWorktreePath: string | null;
  sessions: Record<string, TerminalSession>;
  layout: LayoutState;
  unreadTabIds: Record<string, boolean>;
  unreadWorktreePaths: Record<string, boolean>;
};

export type WorkspaceServices = {
  ensureTerminalEvents: () => Promise<void>;
  spawnTerminal: (request: { workspaceId: string; worktree: WorktreeIdentity | null }) => Promise<string>;
  closeTerminal: (sessionId: string) => Promise<void>;
  waitForTerminalExit: (sessionId: string, timeoutMs: number) => Promise<void>;
};

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
  | {
      type: "SPLIT_PANE";
      tabId: string;
      targetLeafId?: string;
      direction: PaneDirection;
      newLeafId: string;
      session: TerminalSession;
    }
  | {
      type: "CLOSE_PANE";
      tabId: string;
      leafId: string;
    }
  | { type: "FOCUS_PANE"; tabId: string; leafId: string }
  | { type: "SET_PANE_RATIO"; tabId: string; path: string; ratio: number }
  | { type: "SWAP_PANES"; tabId: string; sourceLeafId: string; targetLeafId: string }
  | { type: "SESSION_LIFECYCLE"; backendSessionId: string; lifecycle: TerminalLifecycle }
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

  useEffect(
    () =>
      terminalEventBus.subscribeLifecycle((payload) => {
        dispatch({
          type: "SESSION_LIFECYCLE",
          backendSessionId: payload.sessionId,
          lifecycle: mapBackendLifecycle(payload),
        });
      }),
    [dispatch],
  );

  const createSpawnedTab = useCallback(
    async (worktree: Worktree, label?: string) => {
      await services.ensureTerminalEvents();
      const backendSessionId = await services.spawnTerminal({
        workspaceId,
        worktree: worktreeIdentity(worktree),
      });
      const sessionId = createId("session");
      const tabId = createId("tab");
      const session: TerminalSession = {
        id: sessionId,
        cwd: worktree.path,
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
        (tab) => tab.kind !== "browser" && snapshot.sessions[tab.sessionId]?.cwd === worktree.path,
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
    async (tabId: string, targetLeafId: string, direction: PaneDirection) => {
      const snapshot = stateRef.current;
      const tab = snapshot.layout.tabs.find((t) => t.id === tabId);
      if (!tab || tab.kind === "browser") return;
      const targetSession = snapshot.sessions[tab.sessionId];
      const worktree = targetSession
        ? snapshot.worktrees.find((w) => w.path === targetSession.cwd) ?? getActiveWorktree(snapshot)
        : getActiveWorktree(snapshot);
      if (!worktree) return;

      const newLeafId = createId("leaf");
      dispatch({
        type: "SPLIT_PANE",
        tabId,
        targetLeafId,
        direction,
        newLeafId,
        session: targetSession ?? {
          id: tab.sessionId,
          cwd: worktree.path,
          workspaceId,
          worktree: worktreeIdentity(worktree),
          backendSessionId: null,
          lifecycle: "working",
        },
      });
    },
    [dispatch, workspaceId],
  );

  const closePane = useCallback(
    async (tabId: string, leafId: string) => {
      const snapshot = stateRef.current;
      const tabLayout = snapshot.layout.layoutsByTabId?.[tabId];
      if (!tabLayout) return;
      dispatch({ type: "CLOSE_PANE", tabId, leafId });
    },
    [dispatch],
  );

  const closeTab = useCallback(
    async (tabId: string) => {
      const snapshot = stateRef.current;
      const closingTab = snapshot.layout.tabs.find((tab) => tab.id === tabId);
      if (!closingTab) return;
      if (closingTab.kind === "browser") {
        dispatch({ type: "CLOSE_TAB", tabId });
        return;
      }
      const closingSession = snapshot.sessions[closingTab.sessionId];

      if (snapshot.layout.tabs.length === 1) {
        const worktree =
          (closingSession && snapshot.worktrees.find((candidate) => candidate.path === closingSession.cwd)) ??
          getActiveWorktree(snapshot);
        if (!worktree) return;

        await closeBackendSessionAndWait(closingSession, services);
        const replacement = await createSpawnedTab(worktree, closingTab.label);
        dispatch({ type: "CLOSE_TAB", tabId, replacement });
        return;
      }

      dispatch({ type: "CLOSE_TAB", tabId });
      await closeBackendSession(closingSession, services);
    },
    [createSpawnedTab, dispatch, services],
  );

  const syncWorktrees = useCallback(
    async (worktrees: Worktree[]) => {
      const snapshot = stateRef.current;
      const validWorktreePaths = new Set(worktrees.map((worktree) => worktree.path));
      const staleSessions = Object.values(snapshot.sessions).filter((session) => !validWorktreePaths.has(session.cwd));
      dispatch({ type: "SET_WORKTREES", worktrees });
      await Promise.allSettled(staleSessions.map((session) => closeBackendSession(session, services)));
    },
    [dispatch, services],
  );

  const activateTab = useCallback((tabId: string) => dispatch({ type: "ACTIVATE_TAB", tabId }), [dispatch]);
  const focusPane = useCallback((tabId: string, leafId: string) => dispatch({ type: "FOCUS_PANE", tabId, leafId }), [dispatch]);
  const setPaneRatio = useCallback(
    (tabId: string, path: string, ratio: number) => dispatch({ type: "SET_PANE_RATIO", tabId, path, ratio }),
    [dispatch]
  );
  const swapPanes = useCallback(
    (tabId: string, sourceLeafId: string, targetLeafId: string) => dispatch({ type: "SWAP_PANES", tabId, sourceLeafId, targetLeafId }),
    [dispatch]
  );

  const restoreWorkspace = useCallback(
    (restoredState: WorkspaceState) => dispatch({ type: "RESTORE_WORKSPACE", state: restoredState }),
    [dispatch],
  );

  const createBrowserTab = useCallback(
    async (url = "http://localhost:3000", label?: string) => {
      const activeWorktree = getActiveWorktree(stateRef.current);
      const state = await createBrowser({
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
        browserId: state.browserId,
        url: state.url,
        title: state.title,
        loading: state.loading,
        canGoBack: state.canGoBack,
        canGoForward: state.canGoForward,
      };

      dispatch({ type: "ADD_TAB_WITH_SESSION", tab: browserTab });
      return tabId;
    },
    [workspaceId],
  );

  const navigateBrowserTabAction = useCallback(
    async (tabId: string, url: string) => {
      const tab = stateRef.current.layout.tabs.find((t) => t.id === tabId);
      if (!tab || tab.kind !== "browser") return;
      dispatch({ type: "UPDATE_BROWSER_TAB", tabId, updates: { url, loading: true } });
      await navigateBrowser(tab.browserId, url);
      dispatch({ type: "UPDATE_BROWSER_TAB", tabId, updates: { loading: false } });
    },
    [],
  );

  const reloadBrowserTabAction = useCallback(
    async (tabId: string) => {
      const tab = stateRef.current.layout.tabs.find((t) => t.id === tabId);
      if (!tab || tab.kind !== "browser") return;
      dispatch({ type: "UPDATE_BROWSER_TAB", tabId, updates: { loading: true } });
      await reloadBrowser(tab.browserId);
      dispatch({ type: "UPDATE_BROWSER_TAB", tabId, updates: { loading: false } });
    },
    [],
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
    openTab,
    createBrowserTab,
    navigateBrowserTab: navigateBrowserTabAction,
    reloadBrowserTab: reloadBrowserTabAction,
    ensureTabForWorktree,
    openWorkspacePortInBrowser,
    closeTab,
    splitPane,
    closePane,
    activateTab,
    focusPane,
    setPaneRatio,
    swapPanes,
    syncWorktrees,
    restoreWorkspace,
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
    const worktree = state.worktrees.find((candidate) => candidate.path === session.cwd);
    return [
      {
        id: session.backendSessionId ?? session.id,
        name: tab.label,
        task: worktree?.branch?.replace(/^refs\/heads\//, "") ?? session.cwd,
        state: session.lifecycle,
        worktree: session.worktree,
        worktreePath: worktree?.path ?? session.cwd,
        sessionId: session.backendSessionId ?? session.id,
      },
    ];
  });
}

function createInitialState(worktrees: Worktree[]): WorkspaceState {
  return {
    worktrees,
    activeWorktreePath: worktrees[0]?.path ?? null,
    sessions: {},
    layout: createLayoutState(),
    unreadTabIds: {},
    unreadWorktreePaths: {},
  };
}

function workspaceReducer(state: WorkspaceState, action: WorkspaceAction): WorkspaceState {
  switch (action.type) {
    case "RESTORE_WORKSPACE":
      return action.state;
    case "SET_WORKTREES": {
      const validWorktreePaths = new Set(action.worktrees.map((worktree) => worktree.path));
      const sessions = Object.fromEntries(
        Object.entries(state.sessions).filter(([, session]) => validWorktreePaths.has(session.cwd)),
      );
      let layout = state.layout;
      for (const tab of state.layout.tabs) {
        if (tab.kind !== "browser" && !sessions[tab.sessionId]) {
          layout = layoutReducer(layout, { type: "CLOSE_TAB", tabId: tab.id });
        }
      }
      const activeWorktreePath = action.worktrees.some((worktree) => worktree.path === state.activeWorktreePath)
        ? state.activeWorktreePath
        : action.worktrees[0]?.path ?? null;
      return { ...state, worktrees: action.worktrees, activeWorktreePath, sessions, layout };
    }
    case "UPDATE_BROWSER_TAB": {
      const tabs = state.layout.tabs.map((tab) => {
        if (tab.id === action.tabId && tab.kind === "browser") {
          return { ...tab, ...action.updates };
        }
        return tab;
      });
      return { ...state, layout: { ...state.layout, tabs } };
    }
    case "SELECT_WORKTREE": {
      const unreadWorktreePaths = { ...state.unreadWorktreePaths };
      delete unreadWorktreePaths[action.path];
      return state.worktrees.some((worktree) => worktree.path === action.path)
        ? { ...state, activeWorktreePath: action.path, unreadWorktreePaths }
        : state;
    }
    case "MARK_TAB_UNREAD":
      return {
        ...state,
        unreadTabIds: { ...state.unreadTabIds, [action.tabId]: true },
      };
    case "CLEAR_TAB_UNREAD": {
      const unreadTabIds = { ...state.unreadTabIds };
      delete unreadTabIds[action.tabId];
      return { ...state, unreadTabIds };
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
      const sessions = action.session
        ? { ...state.sessions, [action.session.id]: action.session }
        : state.sessions;
      const sessionId = action.session?.id;
      return {
        ...state,
        sessions,
        layout: layoutReducer(state.layout, { type: "ADD_TAB", tab: action.tab, sessionId }),
      };
    }
    case "SPLIT_PANE":
      return {
        ...state,
        sessions: { ...state.sessions, [action.session.id]: action.session },
        layout: layoutReducer(state.layout, {
          type: "SPLIT_PANE",
          tabId: action.tabId,
          targetLeafId: action.targetLeafId,
          direction: action.direction,
          newLeafId: action.newLeafId,
          sessionId: action.session.id,
        }),
      };
    case "CLOSE_PANE":
      return {
        ...state,
        layout: layoutReducer(state.layout, {
          type: "CLOSE_PANE",
          tabId: action.tabId,
          leafId: action.leafId,
        }),
      };
    case "CLOSE_TAB": {
      const closingTab = state.layout.tabs.find((tab) => tab.id === action.tabId);
      const sessions = { ...state.sessions };
      if (closingTab && closingTab.kind !== "browser") delete sessions[closingTab.sessionId];
      if (action.replacement?.session) sessions[action.replacement.session.id] = action.replacement.session;
      return {
        ...state,
        sessions,
        layout: layoutReducer(state.layout, {
          type: "CLOSE_TAB",
          tabId: action.tabId,
          replacementTab: action.replacement?.tab,
        }),
      };
    }
    case "ACTIVATE_TAB": {
      const unreadTabIds = { ...state.unreadTabIds };
      delete unreadTabIds[action.tabId];
      return { ...state, unreadTabIds, layout: layoutReducer(state.layout, { type: "ACTIVATE_TAB", tabId: action.tabId }) };
    }
    case "FOCUS_PANE":
      return {
        ...state,
        layout: layoutReducer(state.layout, { type: "FOCUS_PANE", tabId: action.tabId, leafId: action.leafId }),
      };
    case "SET_PANE_RATIO":
      return {
        ...state,
        layout: layoutReducer(state.layout, { type: "SET_PANE_RATIO", tabId: action.tabId, path: action.path, ratio: action.ratio }),
      };
    case "SWAP_PANES":
      return {
        ...state,
        layout: layoutReducer(state.layout, { type: "SWAP_PANES", tabId: action.tabId, sourceLeafId: action.sourceLeafId, targetLeafId: action.targetLeafId }),
      };
    case "SESSION_LIFECYCLE": {
      const sessions = Object.fromEntries(
        Object.entries(state.sessions).map(([id, session]) => [
          id,
          session.backendSessionId === action.backendSessionId ? { ...session, lifecycle: action.lifecycle } : session,
        ]),
      );
      return { ...state, sessions };
    }
  }
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
  const base = worktree.path === "." ? "main" : worktree.path.split(/[\\/]/).filter(Boolean).at(-1) ?? worktree.path;
  const count = tabs.filter((tab) => tab.kind !== "browser" && sessions[tab.sessionId]?.cwd === worktree.path).length + 1;
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
