import { useCallback, useEffect, useReducer, useRef } from "react";

import { closeTerminal, spawnTerminal } from "../lib/tauri";
import { ensureTerminalEvents, terminalEventBus } from "../lib/terminalEvents";
import type {
  ActiveAgent,
  LayoutState,
  SplitMode,
  TerminalLifecycle,
  TerminalLifecyclePayload,
  TerminalSession,
  TerminalTab,
  Worktree,
} from "../lib/types";
import { createLayoutState, layoutReducer } from "./layout";

export type WorkspaceState = {
  worktrees: Worktree[];
  activeWorktreeId: string | null;
  sessions: Record<string, TerminalSession>;
  layout: LayoutState;
};

export type WorkspaceServices = {
  ensureTerminalEvents: () => Promise<void>;
  spawnTerminal: (request: { workspaceId: string; worktreeId: string }) => Promise<string>;
  closeTerminal: (sessionId: string) => Promise<void>;
};

type WorkspaceAction =
  | { type: "SET_WORKTREES"; worktrees: Worktree[] }
  | { type: "SELECT_WORKTREE"; worktreeId: string }
  | { type: "ADD_TAB_WITH_SESSION"; tab: TerminalTab; session: TerminalSession }
  | {
      type: "ENABLE_SPLIT_WITH_SESSION";
      tab: TerminalTab;
      session: TerminalSession;
      orientation: Exclude<SplitMode, "none">;
    }
  | { type: "ENABLE_SPLIT_EXISTING"; tabId: string; orientation: Exclude<SplitMode, "none"> }
  | {
      type: "CLOSE_TAB";
      tabId: string;
      replacement?: { tab: TerminalTab; session: TerminalSession };
    }
  | { type: "ACTIVATE_PRIMARY"; tabId: string }
  | { type: "ACTIVATE_SECONDARY"; tabId: string }
  | { type: "ROTATE_SPLIT" }
  | { type: "DISABLE_SPLIT" }
  | { type: "SESSION_LIFECYCLE"; backendSessionId: string; lifecycle: TerminalLifecycle };

type UseWorkspaceStoreOptions = {
  initialWorktrees?: Worktree[];
  services?: WorkspaceServices;
};

const defaultServices: WorkspaceServices = {
  ensureTerminalEvents,
  spawnTerminal,
  closeTerminal,
};

export function useWorkspaceStore({ initialWorktrees = [], services = defaultServices }: UseWorkspaceStoreOptions = {}) {
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
        workspaceId: worktree.wsId,
        worktreeId: worktree.worktreeId,
      });
      const sessionId = createId("session");
      const tabId = createId("tab");
      const session: TerminalSession = {
        id: sessionId,
        cwd: worktree.path,
        workspaceId: worktree.wsId,
        worktreeId: worktree.worktreeId,
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
    [services],
  );

  const openTab = useCallback(
    async (worktree: Worktree) => {
      const binding = await createSpawnedTab(worktree);
      dispatch({ type: "ADD_TAB_WITH_SESSION", ...binding });
      if (!stateRef.current.worktrees.some((candidate) => candidate.worktreeId === worktree.worktreeId)) {
        dispatch({ type: "SET_WORKTREES", worktrees: [...stateRef.current.worktrees, worktree] });
      }
      dispatch({ type: "SELECT_WORKTREE", worktreeId: worktree.worktreeId });
      return binding.tab.id;
    },
    [createSpawnedTab, dispatch],
  );

  const ensureTabForWorktree = useCallback(
    async (worktree: Worktree) => {
      const snapshot = stateRef.current;
      const existing = snapshot.layout.tabs.find((tab) => snapshot.sessions[tab.sessionId]?.worktreeId === worktree.worktreeId);
      dispatch({ type: "SELECT_WORKTREE", worktreeId: worktree.worktreeId });
      if (existing) {
        dispatch({ type: "ACTIVATE_PRIMARY", tabId: existing.id });
        return existing.id;
      }
      return openTab(worktree);
    },
    [dispatch, openTab],
  );

  const enableSplit = useCallback(
    async (orientation: Exclude<SplitMode, "none"> = "horizontal") => {
      let snapshot = stateRef.current;
      if (!snapshot.layout.primaryTabId) {
        const activeWorktree = getActiveWorktree(snapshot);
        if (!activeWorktree) return;
        await openTab(activeWorktree);
        snapshot = stateRef.current;
      }

      const primaryTabId = snapshot.layout.primaryTabId;
      const existingSecondary = snapshot.layout.tabs.find((tab) => tab.id !== primaryTabId);
      if (existingSecondary) {
        dispatch({ type: "ENABLE_SPLIT_EXISTING", tabId: existingSecondary.id, orientation });
        return;
      }

      const primaryTab = snapshot.layout.tabs.find((tab) => tab.id === primaryTabId);
      const primarySession = primaryTab ? snapshot.sessions[primaryTab.sessionId] : undefined;
      const activeWorktree =
        (primarySession && snapshot.worktrees.find((worktree) => worktree.worktreeId === primarySession.worktreeId)) ??
        getActiveWorktree(snapshot);
      if (!activeWorktree) return;

      const binding = await createSpawnedTab(activeWorktree);
      dispatch({ type: "ENABLE_SPLIT_WITH_SESSION", ...binding, orientation });
    },
    [createSpawnedTab, dispatch, openTab],
  );

  const closeTab = useCallback(
    async (tabId: string) => {
      const snapshot = stateRef.current;
      const closingTab = snapshot.layout.tabs.find((tab) => tab.id === tabId);
      if (!closingTab) return;
      const closingSession = snapshot.sessions[closingTab.sessionId];

      let replacement: { tab: TerminalTab; session: TerminalSession } | undefined;
      if (snapshot.layout.tabs.length === 1) {
        const worktree =
          (closingSession && snapshot.worktrees.find((candidate) => candidate.worktreeId === closingSession.worktreeId)) ??
          getActiveWorktree(snapshot);
        if (!worktree) return;
        replacement = await createSpawnedTab(worktree, closingTab.label);
      }

      dispatch({ type: "CLOSE_TAB", tabId, replacement });
      await closeBackendSession(closingSession, services);
    },
    [createSpawnedTab, dispatch, services],
  );

  const syncWorktrees = useCallback(
    async (worktrees: Worktree[]) => {
      const snapshot = stateRef.current;
      const validWorktreeIds = new Set(worktrees.map((worktree) => worktree.worktreeId));
      const staleSessions = Object.values(snapshot.sessions).filter((session) => !validWorktreeIds.has(session.worktreeId));
      dispatch({ type: "SET_WORKTREES", worktrees });
      await Promise.allSettled(staleSessions.map((session) => closeBackendSession(session, services)));
    },
    [dispatch, services],
  );

  const rotateSplit = useCallback(() => dispatch({ type: "ROTATE_SPLIT" }), [dispatch]);
  const disableSplit = useCallback(() => dispatch({ type: "DISABLE_SPLIT" }), [dispatch]);
  const activatePrimary = useCallback((tabId: string) => dispatch({ type: "ACTIVATE_PRIMARY", tabId }), [dispatch]);
  const activateSecondary = useCallback((tabId: string) => dispatch({ type: "ACTIVATE_SECONDARY", tabId }), [dispatch]);

  return {
    state,
    agents: selectAgents(state),
    openTab,
    ensureTabForWorktree,
    closeTab,
    enableSplit,
    rotateSplit,
    disableSplit,
    activatePrimary,
    activateSecondary,
    syncWorktrees,
  };
}

export function selectAgents(state: WorkspaceState): ActiveAgent[] {
  return state.layout.tabs.flatMap((tab) => {
    const session = state.sessions[tab.sessionId];
    if (!session) return [];
    const worktree = state.worktrees.find((candidate) => candidate.worktreeId === session.worktreeId);
    return [
      {
        id: session.backendSessionId ?? session.id,
        name: tab.label,
        task: worktree?.branch?.replace(/^refs\/heads\//, "") ?? session.cwd,
        state: session.lifecycle,
        worktreeId: session.worktreeId,
        worktreePath: worktree?.path ?? session.cwd,
        sessionId: session.backendSessionId ?? session.id,
      },
    ];
  });
}

function createInitialState(worktrees: Worktree[]): WorkspaceState {
  return {
    worktrees,
    activeWorktreeId: worktrees[0]?.worktreeId ?? null,
    sessions: {},
    layout: createLayoutState(),
  };
}

function workspaceReducer(state: WorkspaceState, action: WorkspaceAction): WorkspaceState {
  switch (action.type) {
    case "SET_WORKTREES": {
      const validWorktreeIds = new Set(action.worktrees.map((worktree) => worktree.worktreeId));
      const sessions = Object.fromEntries(
        Object.entries(state.sessions).filter(([, session]) => validWorktreeIds.has(session.worktreeId)),
      );
      let layout = state.layout;
      for (const tab of state.layout.tabs) {
        if (!sessions[tab.sessionId]) layout = layoutReducer(layout, { type: "CLOSE_TAB", tabId: tab.id });
      }
      const activeWorktreeId = action.worktrees.some((worktree) => worktree.worktreeId === state.activeWorktreeId)
        ? state.activeWorktreeId
        : action.worktrees[0]?.worktreeId ?? null;
      return { ...state, worktrees: action.worktrees, activeWorktreeId, sessions, layout };
    }
    case "SELECT_WORKTREE":
      return state.worktrees.some((worktree) => worktree.worktreeId === action.worktreeId)
        ? { ...state, activeWorktreeId: action.worktreeId }
        : state;
    case "ADD_TAB_WITH_SESSION":
      return {
        ...state,
        sessions: { ...state.sessions, [action.session.id]: action.session },
        layout: layoutReducer(state.layout, { type: "ADD_TAB", tab: action.tab }),
      };
    case "ENABLE_SPLIT_WITH_SESSION":
      return {
        ...state,
        sessions: { ...state.sessions, [action.session.id]: action.session },
        layout: layoutReducer(state.layout, {
          type: "ENABLE_SPLIT",
          orientation: action.orientation,
          secondaryTab: action.tab,
        }),
      };
    case "ENABLE_SPLIT_EXISTING":
      return {
        ...state,
        layout: layoutReducer(state.layout, {
          type: "ENABLE_SPLIT",
          orientation: action.orientation,
          secondaryTabId: action.tabId,
        }),
      };
    case "CLOSE_TAB": {
      const closingTab = state.layout.tabs.find((tab) => tab.id === action.tabId);
      const sessions = { ...state.sessions };
      if (closingTab) delete sessions[closingTab.sessionId];
      if (action.replacement) sessions[action.replacement.session.id] = action.replacement.session;
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
    case "ACTIVATE_PRIMARY":
      return { ...state, layout: layoutReducer(state.layout, { type: "ACTIVATE_PRIMARY", tabId: action.tabId }) };
    case "ACTIVATE_SECONDARY":
      return { ...state, layout: layoutReducer(state.layout, { type: "ACTIVATE_SECONDARY", tabId: action.tabId }) };
    case "ROTATE_SPLIT":
      return { ...state, layout: layoutReducer(state.layout, { type: "ROTATE_SPLIT" }) };
    case "DISABLE_SPLIT":
      return { ...state, layout: layoutReducer(state.layout, { type: "DISABLE_SPLIT" }) };
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

function getActiveWorktree(state: WorkspaceState) {
  return state.worktrees.find((worktree) => worktree.worktreeId === state.activeWorktreeId) ?? state.worktrees[0] ?? null;
}

function nextTabLabel(worktree: Worktree, tabs: TerminalTab[], sessions: Record<string, TerminalSession>) {
  const base = worktree.path === "." ? "main" : worktree.path.split(/[\\/]/).filter(Boolean).at(-1) ?? worktree.path;
  const count = tabs.filter((tab) => sessions[tab.sessionId]?.worktreeId === worktree.worktreeId).length + 1;
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
