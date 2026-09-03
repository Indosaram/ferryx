import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";

import {
  activityStateToAgentState,
  summarizeActivities,
  type ActivitySummary,
  type TerminalActivity,
  type TerminalActivityState,
} from "../lib/activity";
import { resolveAgentLogo } from "../lib/agentIcon";
import { agentDisplayNameForType, classifyTerminalTitleActivity, formatTabLabelFromTitle, isBareAgentTitle, normalizeTerminalTitle, parseAgentTitle } from "../lib/agentTitle";
import { workspaceName } from "../lib/branchFilter";
import { closeBrowser, createBrowser, navigateBrowser, reloadBrowser } from "../lib/browserTauri";
import { closeTerminal, DEFAULT_WORKSPACE_ID, discoverAgentProviderSession, getTerminalCwd, onNativeTerminalAgentState, onNativeTerminalBell, onNativeTerminalFocus, onNativeTerminalTitle, spawnTerminal, waitForTerminalExit } from "../lib/tauri";
import { ensureTerminalEvents, terminalEventBus } from "../lib/terminalEvents";
import { switchDebug } from "../lib/switchDebug";
import { worktreeIdentity } from "../lib/types";
import type {
  AgentProviderSession,
  ActiveAgent,
  BrowserTab,
  LayoutState,
  PaneContent,
  ReconnectLifecycle,
  StructuredIpcError,
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
import { moveTabIntoPaneSplit } from "./tabPaneDrop";

const LAST_TAB_EXIT_TIMEOUT_MS = 5_000;

type WorkspaceTerminalActivity = TerminalActivity & {
  agentSource?: "screen" | "title";
};

export type WorkspaceState = {
  workspaceId?: string;
  worktrees: Worktree[];
  activeWorktreePath: string | null;
  sessions: Record<string, TerminalSession>;
  layout: LayoutState;
  worktreeLayouts?: Record<string, LayoutState>;
  unreadTabIds: Record<string, boolean>;
  unreadWorktreePaths: Record<string, boolean>;
  /** Optional for backwards compatibility with persisted/test states created before activity tracking. */
  activityBySessionId?: Record<string, TerminalActivity>;
};

export type WorkspaceServices = {
  ensureTerminalEvents: () => Promise<void>;
  spawnTerminal: (request: {
    workspaceId: string;
    worktree: WorktreeIdentity | null;
    cwd?: string | null;
    clientRequestId?: string | null;
    shell?: string | null;
  }) => Promise<string>;
  getTerminalCwd: (sessionId: string) => Promise<string | null>;
  closeTerminal: (sessionId: string) => Promise<void>;
  waitForTerminalExit: (sessionId: string, timeoutMs: number) => Promise<void>;
};

export type SplitPaneOptions = {
  position?: "first" | "second";
  content?: PaneContent;
};

export type TabSplitEdge = "left" | "right" | "top" | "bottom";

export {
  clearHmrWorkspaceState,
  getHmrWorkspaceState,
  setHmrWorkspaceState,
  type HmrWorkspaceStoreData,
} from "./hmrWorkspaceState";

import { getHmrWorkspaceState, setHmrWorkspaceState } from "./hmrWorkspaceState";
import { getWorkspaceSnapshot, listWorkspaceSnapshots, setWorkspaceSnapshot } from "./workspaceSnapshotCache";

export type WorkspaceAction =
  | { type: "SET_WORKTREES"; worktrees: Worktree[] }
  | { type: "RESTORE_WORKSPACE"; state: WorkspaceState }
  | { type: "SELECT_WORKTREE"; path: string }
  | { type: "ADD_TAB_WITH_SESSION"; tab: WorkspaceTab; session?: TerminalSession; targetWorktreePath?: string }
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
      content?: PaneContent;
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
      type: "MOVE_TAB_TO_PANE_SPLIT";
      sourceTabId: string;
      targetTabId: string;
      targetLeafId: string;
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
  | {
      type: "SET_RECONNECT_LIFECYCLE";
      sessionId: string;
      lifecycle: ReconnectLifecycle;
      error?: StructuredIpcError | null;
      requestId?: string | null;
    }
  | { type: "APPLY_PROVIDER_SESSION_IF_MISSING"; sessionId: string; providerSession: AgentProviderSession; agentType?: string }
  | { type: "REBIND_SESSION_BACKEND"; sessionId: string; backendSessionId: string; cwd?: string; daemonEpoch?: string | null }
  | { type: "SESSION_TITLE_ACTIVITY"; tabId: string; sessionId: string; title: string }
  | {
      type: "SESSION_SCREEN_ACTIVITY";
      tabId: string;
      sessionId: string;
      state: "working" | "blocked" | "idle";
      ruleId: string;
      manifestId?: string;
      providerSession?: AgentProviderSession | null;
    }
  | { type: "MARK_TAB_UNREAD"; tabId: string }
  | { type: "CLEAR_TAB_UNREAD"; tabId: string }
  | { type: "MARK_WORKTREE_UNREAD"; worktreePath: string }
  | { type: "CLEAR_WORKTREE_UNREAD"; worktreePath: string }
  | { type: "MARK_SESSION_ACTIVITY_SEEN"; sessionId: string };

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
  const initialRef = useRef<{ state: WorkspaceState; recoveredFromHmr: boolean } | null>(null);
  if (!initialRef.current) {
    initialRef.current = initWorkspaceState({ workspaceId, initialWorktrees });
  }
  const [state, reactDispatch] = useReducer(workspaceReducer, initialRef.current.state);
  const stateRef = useRef(state);
  const spawningSessionIdsRef = useRef(new Set<string>());
  /**
   * Bell subscribers keyed by nothing: the native bell event is global, so App registers one
   * listener here instead of a per-pane prop that only exists for the mounted foreground tab.
   */
  const bellListenersRef = useRef(new Set<(sessionId: string, tabId: string) => void>());
  const mountedWorkspaceIdRef = useRef(workspaceId);
  // Snapshots of parked workspaces live outside React state, so a change there has to be announced
  // for the sidebar's cross-workspace activity to recompute.
  const [parkedActivityVersion, setParkedActivityVersion] = useState(0);
  const bumpParkedActivity = useCallback(() => setParkedActivityVersion((value) => value + 1), []);
  // Provenance belongs to the workspace currently mounted; keeping the first
  // one would make restore pick the wrong HMR-vs-disk path after a switch.
  const recoveredFromHmrRef = useRef(initialRef.current.recoveredFromHmr);

  let renderedState = state;
  let recoveredFromHmr = recoveredFromHmrRef.current;

  if (state.workspaceId !== workspaceId) {
    const previousWorkspaceId = state.workspaceId ?? mountedWorkspaceIdRef.current;
    setWorkspaceSnapshot(previousWorkspaceId, state);
    const swapped = initWorkspaceState({ workspaceId, initialWorktrees });
    switchDebug("workspace.store.swap", {
      fromWorkspaceId: previousWorkspaceId,
      toWorkspaceId: workspaceId,
      outgoingTabCount: state.layout.tabs.length,
      outgoingSessionCount: Object.keys(state.sessions).length,
      incomingStateWorkspaceId: swapped.state.workspaceId ?? null,
      incomingActiveWorktreePath: swapped.state.activeWorktreePath,
      incomingWorktreeCount: swapped.state.worktrees.length,
      incomingTabCount: swapped.state.layout.tabs.length,
      incomingSessionCount: Object.keys(swapped.state.sessions).length,
      recoveredFromHmr: swapped.recoveredFromHmr,
    });
    renderedState = swapped.state;
    recoveredFromHmr = swapped.recoveredFromHmr;
    recoveredFromHmrRef.current = swapped.recoveredFromHmr;
    reactDispatch({ type: "RESTORE_WORKSPACE", state: swapped.state });
  }

  stateRef.current = renderedState;
  mountedWorkspaceIdRef.current = workspaceId;

  const dispatch = useCallback(
    (action: WorkspaceAction) => {
      const nextState = workspaceReducer(stateRef.current, action);
      stateRef.current = nextState;
      const owningWorkspaceId = nextState.workspaceId ?? mountedWorkspaceIdRef.current;
      setHmrWorkspaceState(owningWorkspaceId, nextState);
      setWorkspaceSnapshot(owningWorkspaceId, nextState);
      reactDispatch(action);
    },
    [],
  );

  useEffect(() => {
    const owningWorkspaceId = renderedState.workspaceId ?? workspaceId;
    setHmrWorkspaceState(owningWorkspaceId, renderedState);
  }, [renderedState, workspaceId]);

  useEffect(() => {
    const unsubscribeLifecycle = terminalEventBus.subscribeLifecycle((payload) => {
      dispatch({
        type: "SESSION_LIFECYCLE",
        backendSessionId: payload.sessionId,
        lifecycle: mapBackendLifecycle(payload),
      });
    });

    // Native title and bell events carry the BACKEND session id and are emitted by the daemon
    // stream pump, so they arrive for every attached session -- including background tabs whose
    // panes `TerminalSplitView` has unmounted. That is the whole point: the notification exists
    // for the tab the user is NOT watching.
    const locateSession = (state: WorkspaceState, backendSessionId: string) => {
      const session = Object.values(state.sessions).find(
        (candidate) => candidate.backendSessionId === backendSessionId,
      );
      if (!session) return null;
      const tabId = findTabIdForSession(state, session.id);
      if (!tabId) return null;
      return { sessionId: session.id, tabId };
    };

    const resolveSession = (backendSessionId: string) =>
      locateSession(stateRef.current, backendSessionId);

    /**
     * Applies an activity action to a session owned by a project that is not currently mounted.
     *
     * Only one workspace has a live reducer, so without this an agent working in another project
     * would freeze at whatever state it held when the user switched away -- and the sidebar row
     * that now reads snapshots would spin forever. Returns true when the event was consumed.
     */
    const dispatchToParkedWorkspace = (
      backendSessionId: string,
      build: (resolved: { sessionId: string; tabId: string }) => WorkspaceAction,
    ): boolean => {
      const mountedWorkspaceId = stateRef.current.workspaceId ?? mountedWorkspaceIdRef.current;
      for (const [snapshotWorkspaceId, snapshot] of listWorkspaceSnapshots()) {
        if (snapshotWorkspaceId === mountedWorkspaceId) continue;
        const resolved = locateSession(snapshot, backendSessionId);
        if (!resolved) continue;
        const nextState = workspaceReducer(snapshot, build(resolved));
        if (nextState === snapshot) return true;
        setWorkspaceSnapshot(snapshotWorkspaceId, nextState);
        bumpParkedActivity();
        return true;
      }
      return false;
    };

    let unlistenTitle: (() => void) | undefined;
    let unlistenBell: (() => void) | undefined;
    let unlistenAgentState: (() => void) | undefined;
    let unlistenFocus: (() => void) | undefined;
    const fallbackAttemptedAgents = new Map<string, Set<string>>();
    let subscribed = true;

    void onNativeTerminalTitle((payload) => {
      const resolved = resolveSession(payload.sessionId);
      if (!resolved) {
        dispatchToParkedWorkspace(payload.sessionId, (parked) => ({
          type: "SESSION_TITLE_ACTIVITY",
          tabId: parked.tabId,
          sessionId: parked.sessionId,
          title: payload.title,
        }));
        return;
      }
      dispatch({ type: "SESSION_TITLE_ACTIVITY", tabId: resolved.tabId, sessionId: resolved.sessionId, title: payload.title });
    })
      .then((unlisten) => {
        if (subscribed) unlistenTitle = unlisten;
        else unlisten();
      })
      .catch(() => undefined);

    void onNativeTerminalBell((payload) => {
      const resolved = resolveSession(payload.sessionId);
      if (!resolved) return;
      bellListenersRef.current.forEach((listener) => listener(resolved.sessionId, resolved.tabId));
    })
      .then((unlisten) => {
        if (subscribed) unlistenBell = unlisten;
        else unlisten();
      })
      .catch(() => undefined);

    void onNativeTerminalAgentState((payload) => {
      const resolved = resolveSession(payload.sessionId);
      if (!resolved) {
        dispatchToParkedWorkspace(payload.sessionId, (parked) => ({
          type: "SESSION_SCREEN_ACTIVITY",
          tabId: parked.tabId,
          sessionId: parked.sessionId,
          state: payload.state,
          ruleId: payload.ruleId,
          manifestId: payload.manifestId,
          providerSession: payload.providerSession,
        }));
        return;
      }
      dispatch({
        type: "SESSION_SCREEN_ACTIVITY",
        tabId: resolved.tabId,
        sessionId: resolved.sessionId,
        state: payload.state,
        ruleId: payload.ruleId,
        manifestId: payload.manifestId,
        providerSession: payload.providerSession,
      });
      if (
        !payload.providerSession
        && payload.manifestId
        && ["claude", "codex", "copilot", "cursor", "cursor-agent", "kimi", "omo", "gjc", "antigravity"].includes(payload.manifestId)
      ) {
        const discoveryAgent = payload.manifestId === "cursor-agent" ? "cursor" : payload.manifestId;
        // A pane can surface several manifest ids over its lifetime (shared TUI
        // patterns across pi-family agents), so track attempts per agent id: a
        // failed probe under one id must not block a more specific id later.
        const attempted = fallbackAttemptedAgents.get(resolved.sessionId) ?? new Set<string>();
        if (!attempted.has(discoveryAgent)) {
          attempted.add(discoveryAgent);
          fallbackAttemptedAgents.set(resolved.sessionId, attempted);
          void discoverAgentProviderSession(payload.sessionId, discoveryAgent).then((id) => {
            if (!id) return;
            dispatch({
              type: "APPLY_PROVIDER_SESSION_IF_MISSING",
              sessionId: resolved.sessionId,
              providerSession: { key: "session_id", id },
              agentType: discoveryAgent,
            });
          });
        }
      }
    })
      .then((unlisten) => {
        if (subscribed) unlistenAgentState = unlisten;
        else unlisten();
      })
      .catch(() => undefined);

    void onNativeTerminalFocus((backendSessionId) => {
      const resolved = resolveSession(backendSessionId);
      if (!resolved) return;
      dispatch({ type: "MARK_SESSION_ACTIVITY_SEEN", sessionId: resolved.sessionId });
    })
      .then((unlisten) => {
        if (subscribed) unlistenFocus = unlisten;
        else unlisten();
      })
      .catch(() => undefined);

    return () => {
      subscribed = false;
      unsubscribeLifecycle();
      unlistenTitle?.();
      unlistenBell?.();
      unlistenAgentState?.();
      unlistenFocus?.();
    };
  }, [dispatch]);

  const createSpawnedTab = useCallback(
    async (worktree: Worktree, label?: string, backendSessionIdOverride?: string, shell?: string) => {
      await services.ensureTerminalEvents();
      const backendSessionId =
        backendSessionIdOverride ??
        (await spawnTerminalForLogicalAction(services, {
          workspaceId,
          worktree: worktreeIdentity(worktree),
          cwd: worktree.path,
          shell,
        }));
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
        label: label ?? nextTabLabel(worktree, getAllTabs(stateRef.current), stateRef.current.sessions),
        sessionId,
      };
      return { tab, session };
    },
    [services, workspaceId],
  );

  const openTab = useCallback(
    async (worktree: Worktree, label?: string, backendSessionIdOverride?: string, shell?: string) => {
      const capturedWorktreePath = worktree.path;
      switchDebug("terminal.open.start", {
        workspaceId,
        worktreePath: capturedWorktreePath,
        tabCount: stateRef.current.layout.tabs.length,
        sessionCount: Object.keys(stateRef.current.sessions).length,
        backendOverride: backendSessionIdOverride ?? null,
      });
      const binding = await createSpawnedTab(worktree, label, backendSessionIdOverride, shell);
      // The active project can change while the spawn is in flight; landing this
      // tab now would inject one project's worktree into another's state, and
      // dropping it silently would orphan the backend PTY we just created.
      if (mountedWorkspaceIdRef.current !== workspaceId) {
        switchDebug("terminal.open.discarded", {
          requestedWorkspaceId: workspaceId,
          mountedWorkspaceId: mountedWorkspaceIdRef.current,
          worktreePath: capturedWorktreePath,
          backendSessionId: binding.session.backendSessionId,
        });
        if (backendSessionIdOverride === undefined) {
          await closeBackendSession(binding.session, services);
        }
        return null;
      }
      if (!stateRef.current.worktrees.some((candidate) => candidate.path === capturedWorktreePath)) {
        dispatch({ type: "SET_WORKTREES", worktrees: [...stateRef.current.worktrees, worktree] });
      }
      dispatch({ type: "ADD_TAB_WITH_SESSION", ...binding, targetWorktreePath: capturedWorktreePath });
      dispatch({ type: "SELECT_WORKTREE", path: capturedWorktreePath });
      switchDebug("terminal.open.complete", {
        workspaceId,
        worktreePath: capturedWorktreePath,
        tabId: binding.tab.id,
        sessionId: binding.session.id,
        backendSessionId: binding.session.backendSessionId,
        tabCount: stateRef.current.layout.tabs.length,
      });
      return binding.tab.id;
    },
    [createSpawnedTab, dispatch, services, workspaceId],
  );

  const ensureSessionBackends = useCallback(
    async (sessionIds: string[]) => {
      const targets = sessionIds.filter((sessionId) => {
        const session = stateRef.current.sessions[sessionId];
        if (!session || session.backendSessionId != null) return false;
        if (session.lifecycle === "exited" && (session.agentType || session.providerSession || session.agentSessionId)) {
          return false;
        }
        if (spawningSessionIdsRef.current.has(sessionId)) return false;
        return true;
      });
      if (targets.length === 0) return;

      for (const sessionId of targets) {
        spawningSessionIdsRef.current.add(sessionId);
      }

      await Promise.all(
        targets.map(async (sessionId) => {
          try {
            const session = stateRef.current.sessions[sessionId];
            if (!session || session.backendSessionId != null) return;
            await services.ensureTerminalEvents();
            const backendSessionId = await services.spawnTerminal({
              workspaceId,
              worktree: session.worktree,
              cwd: session.cwd ?? session.worktreePath,
            });
            // Recovery spawns can outlive a project switch; rebinding now would
            // point another project's session at this PTY.
            if (mountedWorkspaceIdRef.current !== workspaceId) {
              await closeBackendSession({ ...session, backendSessionId }, services);
              return;
            }
            dispatch({
              type: "REBIND_SESSION_BACKEND",
              sessionId,
              backendSessionId,
            });
          } catch {
            // Spawn failed; allow subsequent attempts
          } finally {
            spawningSessionIdsRef.current.delete(sessionId);
          }
        }),
      );
    },
    [dispatch, services, workspaceId],
  );

  const ensureTabForWorktree = useCallback(
    async (worktree: Worktree, options?: { allowCreate?: boolean }) => {
      const allowCreate = options?.allowCreate ?? true;
      const snapshot = stateRef.current;
      switchDebug("worktree.ensure.start", {
        workspaceId,
        requestedPath: worktree.path,
        activeWorktreePath: snapshot.activeWorktreePath,
        allowCreate,
        activeTabCount: snapshot.layout.tabs.length,
        activeTabIds: snapshot.layout.tabs.map((tab) => tab.id),
        parkedTabCount: snapshot.worktreeLayouts?.[worktree.path]?.tabs.length ?? 0,
        sessionCount: Object.keys(snapshot.sessions).length,
      });
      if (snapshot.activeWorktreePath === worktree.path) {
        const hasValidActiveTab = Boolean(
          snapshot.layout.activeTabId &&
            snapshot.layout.tabs.some((tab) => tab.id === snapshot.layout.activeTabId),
        );
        const activeTab = snapshot.layout.tabs.find(
          (tab) => tab.kind !== "browser" && sessionWorktreePath(snapshot.sessions[tab.sessionId]) === worktree.path,
        ) ?? snapshot.layout.tabs[0];
        if (activeTab) {
          if (hasValidActiveTab) {
            switchDebug("worktree.ensure.skipped", {
              workspaceId,
              worktreePath: worktree.path,
              activeTabId: snapshot.layout.activeTabId,
            });
            return snapshot.layout.activeTabId;
          }
          switchDebug("worktree.ensure.active-tab", {
            workspaceId,
            worktreePath: worktree.path,
            tabId: activeTab.id,
          });
          dispatch({ type: "ACTIVATE_TAB", tabId: activeTab.id });
          return activeTab.id;
        }
        if (!allowCreate) {
          switchDebug("worktree.ensure.empty-no-create", {
            workspaceId,
            worktreePath: worktree.path,
          });
          return null;
        }
        switchDebug("worktree.ensure.create-active", {
          workspaceId,
          worktreePath: worktree.path,
        });
        return openTab(worktree);
      }

      const parkedLayout = snapshot.worktreeLayouts?.[worktree.path];
      if (parkedLayout && parkedLayout.tabs.length > 0) {
        switchDebug("worktree.ensure.restore-parked", {
          workspaceId,
          worktreePath: worktree.path,
          parkedTabCount: parkedLayout.tabs.length,
          parkedActiveTabId: parkedLayout.activeTabId,
        });
        dispatch({ type: "SELECT_WORKTREE", path: worktree.path });
        const activeTabId = parkedLayout.activeTabId ?? parkedLayout.tabs[0]?.id;
        if (activeTabId) {
          dispatch({ type: "ACTIVATE_TAB", tabId: activeTabId });
          return activeTabId;
        }
        return parkedLayout.tabs[0].id;
      }

      const existingInCurrent = snapshot.layout.tabs.find(
        (tab) => tab.kind !== "browser" && sessionWorktreePath(snapshot.sessions[tab.sessionId]) === worktree.path,
      );
      if (existingInCurrent) {
        switchDebug("worktree.ensure.existing-current", {
          workspaceId,
          worktreePath: worktree.path,
          tabId: existingInCurrent.id,
        });
        dispatch({ type: "SELECT_WORKTREE", path: worktree.path });
        dispatch({ type: "ACTIVATE_TAB", tabId: existingInCurrent.id });
        return existingInCurrent.id;
      }

      switchDebug("worktree.ensure.create-new", {
        workspaceId,
        worktreePath: worktree.path,
      });
      return openTab(worktree);
    },
    [dispatch, openTab, workspaceId],
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

      const localSessionId = createId("session");
      const newLeafId = createId("leaf");
      const session: TerminalSession = {
        id: localSessionId,
        cwd: sourceSession.cwd,
        worktreePath: sessionWorktreePath(sourceSession),
        workspaceId: sourceSession.workspaceId || workspaceId,
        worktree: sourceSession.worktree,
        backendSessionId: null,
        lifecycle: "working",
      };

      dispatch({
        type: "SPLIT_PANE",
        tabId: targetTab.id,
        targetLeafId,
        direction,
        position: options.position,
        newLeafId,
        content: options.content,
        session,
      });

      if (options.content && options.content.kind !== "terminal") {
        return;
      }

      let backendSessionId: string | null = null;
      try {
        await services.ensureTerminalEvents();
        let inheritedCwd = sourceSession.cwd;
        if (sourceSession.backendSessionId) {
          try {
            inheritedCwd = (await services.getTerminalCwd(sourceSession.backendSessionId)) ?? sourceSession.cwd;
          } catch {
            inheritedCwd = sourceSession.cwd;
          }
        }

        backendSessionId = await spawnTerminalForLogicalAction(services, {
          workspaceId,
          worktree: sourceSession.worktree,
          cwd: inheritedCwd,
        });

        if (stateRef.current.sessions[localSessionId]) {
          dispatch({
            type: "REBIND_SESSION_BACKEND",
            sessionId: localSessionId,
            backendSessionId,
            cwd: inheritedCwd,
          });
        } else {
          terminalEventBus.clearSession(backendSessionId);
          await services.closeTerminal(backendSessionId).catch(() => undefined);
        }
      } catch (error) {
        if (backendSessionId) {
          terminalEventBus.clearSession(backendSessionId);
          await services.closeTerminal(backendSessionId).catch(() => undefined);
        }
        throw error;
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
    (
      sourceTabId: string,
      targetGroupId: string,
      direction: PaneDirection,
      position: "first" | "second" = "second",
      targetPane?: { tabId: string; leafId: string },
    ) => {
      if (targetPane) {
        dispatch({
          type: "MOVE_TAB_TO_PANE_SPLIT",
          sourceTabId,
          targetTabId: targetPane.tabId,
          targetLeafId: targetPane.leafId,
          direction,
          position,
        });
      } else {
        dispatch({ type: "MOVE_TAB_TO_SPLIT", sourceTabId, targetGroupId, direction, position });
      }
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

      const disposableSessions = getDisposableSessionsForTab(snapshot, tabId);
      if (snapshot.layout.tabs.length === 1) {
        await Promise.all(
          disposableSessions.map((session) => closeBackendSessionAndWait(session, services)),
        );
        dispatch({ type: "CLOSE_TAB", tabId });
        return;
      }

      dispatch({ type: "CLOSE_TAB", tabId });
      await Promise.allSettled(disposableSessions.map((session) => closeBackendSession(session, services)));
    },
    [dispatch, services],
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
      const tab = getAllTabs(snapshot).find((candidate) => candidate.id === tabId);
      if (!tab || tab.kind === "browser") return;
      const sessionId = explicitSessionId ?? (tab as TerminalTab).sessionId;
      if (!snapshot.sessions[sessionId]) return;
      dispatch({ type: "SESSION_TITLE_ACTIVITY", tabId, sessionId, title });
    },
    [dispatch],
  );

  const subscribeTerminalBell = useCallback((listener: (sessionId: string, tabId: string) => void) => {
    const listeners = bellListenersRef.current;
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }, []);

  const createBrowserTab = useCallback(
    async (url = "http://localhost:3000", label?: string, options?: { worktreePath?: string; profileId?: string; browserId?: string }) => {
      const capturedWorktreePath = options?.worktreePath ?? stateRef.current.activeWorktreePath ?? undefined;
      const targetWorktree = capturedWorktreePath
        ? stateRef.current.worktrees.find((wt) => wt.path === capturedWorktreePath)
        : getActiveWorktree(stateRef.current);
      const browserState = await createBrowser({
        browserId: options?.browserId,
        workspaceId,
        worktreePath: targetWorktree?.path,
        url,
        profile: options?.profileId,
        visible: true,
      });
      // Landing this tab after a project switch would attach one project's
      // browser to another's layout, and dropping it silently would orphan the
      // webview we just created.
      if (mountedWorkspaceIdRef.current !== workspaceId) {
        await closeBrowser(browserState.browserId).catch(() => undefined);
        return null;
      }

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
        zoomFactor: browserState.zoomFactor,
        loadError: browserState.loadError ?? null,
        profileId: browserState.profileId,
        worktreePath: targetWorktree?.path,
      };

      dispatch({ type: "ADD_TAB_WITH_SESSION", tab: browserTab, targetWorktreePath: capturedWorktreePath });
      return tabId;
    },
    [dispatch, workspaceId],
  );

  const duplicateBrowserTab = useCallback(
    async (tabId: string, profileId?: string) => {
      const source = stateRef.current.layout.tabs.find((tab) => tab.id === tabId);
      if (!source || source.kind !== "browser") return null;
      return createBrowserTab(source.url, source.label, {
        worktreePath: source.worktreePath,
        profileId: profileId ?? source.profileId,
      });
    },
    [createBrowserTab],
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

  const agents = useMemo(() => selectAgents(renderedState), [renderedState]);
  const tabActivity = useMemo(() => selectTabActivitySummaries(renderedState), [renderedState]);
  // Rows for other projects come from their snapshots and are overlaid by the live workspace, so a
  // worktree the user switched away from keeps reporting its agent instead of going blank.
  const worktreeActivity = useMemo(
    () => ({
      ...selectWorktreeActivitySummariesAcrossWorkspaces(renderedState.workspaceId ?? workspaceId),
      ...selectWorktreeActivitySummaries(renderedState),
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- parkedActivityVersion is the change
    // signal for the snapshot cache, which useMemo cannot observe directly.
    [renderedState, workspaceId, parkedActivityVersion],
  );
  const activityNotificationTargets = useMemo(
    () => selectActivityNotificationTargets(renderedState),
    [renderedState],
  );

  return {
    state: renderedState,
    recoveredFromHmr,
    agents,
    tabActivity,
    worktreeActivity,
    activityNotificationTargets,
    openTab,
    createBrowserTab,
    duplicateBrowserTab,
    navigateBrowserTab: navigateBrowserTabAction,
    reloadBrowserTab: reloadBrowserTabAction,
    ensureTabForWorktree,
    openWorkspacePortInBrowser,
    ensureSessionBackends,
    dispatchWorkspaceAction: dispatch,
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
    subscribeTerminalBell,
    markTabUnread: (tabId: string) => dispatch({ type: "MARK_TAB_UNREAD", tabId }),
    clearTabUnread: (tabId: string) => dispatch({ type: "CLEAR_TAB_UNREAD", tabId }),
    markWorktreeUnread: (worktreePath: string) => dispatch({ type: "MARK_WORKTREE_UNREAD", worktreePath }),
    clearWorktreeUnread: (worktreePath: string) => dispatch({ type: "CLEAR_WORKTREE_UNREAD", worktreePath }),
  };
}

export function selectAgents(state: WorkspaceState): ActiveAgent[] {
  const allTabs = getAllTabs(state);
  return allTabs.flatMap((tab) => {
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
        state: activity
          ? activityStateToAgentState(activity.state)
          : session.lifecycle === "running"
            ? "working"
            : session.lifecycle,
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
  const allTabs = getAllTabs(state);

  for (const tab of allTabs) {
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
  const allTabs = getAllTabs(state);

  for (const tab of allTabs) {
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

/**
 * Worktree activity for every project EXCEPT the mounted one.
 *
 * The store is per project: `useWorkspaceStore({ workspaceId })` mounts one workspace at a time, so
 * an agent running in another project has no live store to feed the sidebar. Its rows would sit
 * blank while the agent works, then light up again the moment the user switched back. The snapshot
 * cache already holds each workspace's last state, which is what the sidebar reads here.
 *
 * The mounted workspace is excluded on purpose: its snapshot lags the live reducer state by a
 * render, and the caller merges this map UNDER the live one.
 */
export function selectWorktreeActivitySummariesAcrossWorkspaces(
  mountedWorkspaceId: string,
): Record<string, ActivitySummary> {
  const merged: Record<string, ActivitySummary> = {};

  for (const [workspaceId, snapshot] of listWorkspaceSnapshots()) {
    if (workspaceId === mountedWorkspaceId) continue;
    for (const [path, summary] of Object.entries(selectWorktreeActivitySummaries(snapshot))) {
      merged[path] = summary;
    }
  }

  return merged;
}

export type ActivityNotificationTarget = {
  sessionId: string;
  tabId: string;
  worktreePath: string;
  worktreeLabel: string;
  agentLabel: string | undefined;
  terminalTitle: string;
  state: TerminalActivityState;
};

export function selectActivityNotificationTargets(state: WorkspaceState): ActivityNotificationTarget[] {
  const targets: ActivityNotificationTarget[] = [];
  const activityBySessionId = state.activityBySessionId ?? {};

  for (const [sessionId, activity] of Object.entries(activityBySessionId)) {
    const tabId = findTabIdForSession(state, sessionId);
    if (!tabId) continue;
    const session = state.sessions[sessionId];
    const worktreePath = sessionWorktreePath(session);
    const worktree = state.worktrees.find((candidate) => candidate.path === worktreePath);
    const worktreeLabel = worktree ? workspaceName(worktree) : "";
    const parsed = parseAgentTitle(activity.title);
    // An extension-reported state carries agentType but no agent name in the title, so fall back
    // to the classified type rather than sending a nameless notification.
    const agentLabel = parsed?.isAgent
      ? parsed.name
      : agentDisplayNameForType(activity.isAgent ? activity.agentType : undefined);

    targets.push({
      sessionId,
      tabId,
      worktreePath,
      worktreeLabel,
      agentLabel,
      terminalTitle: activity.title,
      state: activity.state,
    });
  }

  return targets;
}

function initWorkspaceState({
  workspaceId,
  initialWorktrees,
}: {
  workspaceId: string;
  initialWorktrees: Worktree[];
}): { state: WorkspaceState; recoveredFromHmr: boolean } {
  const hmrState = getHmrWorkspaceState(workspaceId);
  if (hmrState) {
    switchDebug("workspace.state.init", {
      workspaceId,
      source: "hmr",
      worktreeCount: hmrState.worktrees.length,
      tabCount: hmrState.layout.tabs.length,
      sessionCount: Object.keys(hmrState.sessions).length,
    });
    return { state: { ...hmrState, workspaceId }, recoveredFromHmr: true };
  }
  const snapshot = getWorkspaceSnapshot(workspaceId);
  if (snapshot) {
    switchDebug("workspace.state.init", {
      workspaceId,
      source: "snapshot",
      worktreeCount: snapshot.worktrees.length,
      tabCount: snapshot.layout.tabs.length,
      sessionCount: Object.keys(snapshot.sessions).length,
    });
    return { state: { ...snapshot, workspaceId }, recoveredFromHmr: false };
  }
  switchDebug("workspace.state.init", {
    workspaceId,
    source: "empty",
    initialWorktreeCount: initialWorktrees.length,
  });
  return { state: createInitialState(initialWorktrees, workspaceId), recoveredFromHmr: false };
}

function createInitialState(worktrees: Worktree[], workspaceId?: string): WorkspaceState {
  return {
    workspaceId,
    worktrees,
    activeWorktreePath: worktrees[0]?.path ?? null,
    sessions: {},
    layout: createLayoutState(),
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  };
}

export function workspaceReducer(state: WorkspaceState, action: WorkspaceAction): WorkspaceState {
  switch (action.type) {
    case "RESTORE_WORKSPACE":
      return {
        ...action.state,
        workspaceId: action.state.workspaceId ?? state.workspaceId,
        worktreeLayouts: action.state.worktreeLayouts ?? {},
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
        const hasRetainedSession = [...getTabSessionIdsForLayout(state.layout, tab.id)].some((sessionId) => Boolean(sessions[sessionId]));
        if (tab.kind !== "browser" && !hasRetainedSession) {
          layout = layoutReducer(layout, { type: "CLOSE_TAB", tabId: tab.id });
        }
      }
      const worktreeLayouts: Record<string, LayoutState> = {};
      for (const [path, parkedLayout] of Object.entries(state.worktreeLayouts ?? {})) {
        if (!validWorktreePaths.has(path)) continue;
        let cleanedLayout = parkedLayout;
        for (const tab of parkedLayout.tabs) {
          const hasRetainedSession = [...getTabSessionIdsForLayout(parkedLayout, tab.id)].some((sessionId) => Boolean(sessions[sessionId]));
          if (tab.kind !== "browser" && !hasRetainedSession) {
            cleanedLayout = layoutReducer(cleanedLayout, { type: "CLOSE_TAB", tabId: tab.id });
          }
        }
        worktreeLayouts[path] = cleanedLayout;
      }
      let activeWorktreePath = state.activeWorktreePath;
      if (!action.worktrees.some((worktree) => worktree.path === activeWorktreePath)) {
        activeWorktreePath = action.worktrees[0]?.path ?? null;
        if (activeWorktreePath && worktreeLayouts[activeWorktreePath]) {
          layout = worktreeLayouts[activeWorktreePath];
          delete worktreeLayouts[activeWorktreePath];
        }
      }
      const allTabs = [...layout.tabs, ...Object.values(worktreeLayouts).flatMap((l) => l.tabs)];
      const unreadTabIds = Object.fromEntries(
        Object.entries(state.unreadTabIds).filter(([tabId]) => allTabs.some((tab) => tab.id === tabId)),
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
        worktreeLayouts,
        activityBySessionId,
        unreadTabIds,
        unreadWorktreePaths,
      };
    }
    case "UPDATE_BROWSER_TAB": {
      const inActive = state.layout.tabs.some((tab) => tab.id === action.tabId && tab.kind === "browser");
      if (inActive) {
        const tabs = state.layout.tabs.map((tab) => {
          if (tab.id === action.tabId && tab.kind === "browser") return { ...tab, ...action.updates };
          return tab;
        });
        return { ...state, layout: { ...state.layout, tabs } };
      }
      const worktreeLayouts = { ...(state.worktreeLayouts ?? {}) };
      for (const [wtPath, parkedLayout] of Object.entries(worktreeLayouts)) {
        if (parkedLayout.tabs.some((tab) => tab.id === action.tabId && tab.kind === "browser")) {
          const tabs = parkedLayout.tabs.map((tab) => {
            if (tab.id === action.tabId && tab.kind === "browser") return { ...tab, ...action.updates };
            return tab;
          });
          worktreeLayouts[wtPath] = { ...parkedLayout, tabs };
        }
      }
      return { ...state, worktreeLayouts };
    }
    case "SELECT_WORKTREE": {
      if (!state.worktrees.some((worktree) => worktree.path === action.path)) return state;
      if (action.path === state.activeWorktreePath) {
        const unreadWorktreePaths = { ...state.unreadWorktreePaths };
        delete unreadWorktreePaths[action.path];
        const unreadTabIds = { ...state.unreadTabIds };
        for (const tab of state.layout.tabs) {
          delete unreadTabIds[tab.id];
        }
        return { ...state, unreadTabIds, unreadWorktreePaths };
      }

      const nextWorktreeLayouts = { ...(state.worktreeLayouts ?? {}) };
      if (state.activeWorktreePath) {
        nextWorktreeLayouts[state.activeWorktreePath] = state.layout;
      }
      const nextLayout = nextWorktreeLayouts[action.path] ?? createLayoutState();
      delete nextWorktreeLayouts[action.path];

      const unreadWorktreePaths = { ...state.unreadWorktreePaths };
      delete unreadWorktreePaths[action.path];
      const unreadTabIds = { ...state.unreadTabIds };
      for (const tab of nextLayout.tabs) {
        delete unreadTabIds[tab.id];
      }
      return {
        ...state,
        activeWorktreePath: action.path,
        layout: nextLayout,
        worktreeLayouts: nextWorktreeLayouts,
        unreadTabIds,
        unreadWorktreePaths,
      };
    }
    case "MARK_TAB_UNREAD": {
      if (isTabVisible(state, action.tabId)) return state;
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
      const targetWorktreePath =
        action.targetWorktreePath ??
        action.session?.worktreePath ??
        (action.tab.kind === "browser" ? action.tab.worktreePath : undefined);

      if (targetWorktreePath && state.activeWorktreePath && targetWorktreePath !== state.activeWorktreePath) {
        const targetLayout = state.worktreeLayouts?.[targetWorktreePath] ?? createLayoutState();
        const updatedLayout = layoutReducer(targetLayout, { type: "ADD_TAB", tab: action.tab, sessionId });
        return {
          ...state,
          sessions,
          worktreeLayouts: {
            ...(state.worktreeLayouts ?? {}),
            [targetWorktreePath]: updatedLayout,
          },
        };
      }

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
        content: action.content,
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
    case "MOVE_TAB_TO_PANE_SPLIT": {
      const layout = moveTabIntoPaneSplit(
        state.layout,
        action.sourceTabId,
        action.targetTabId,
        action.targetLeafId,
        action.direction,
        action.position,
      );
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

      const inActiveLayout = state.layout.tabs.some((t) => t.id === action.tabId);
      let layout = state.layout;
      let worktreeLayouts = state.worktreeLayouts;

      if (inActiveLayout) {
        layout = layoutReducer(state.layout, {
          type: "CLOSE_TAB",
          tabId: action.tabId,
          replacementTab: action.replacement?.tab,
        });
      } else if (worktreeLayouts) {
        const nextWorktreeLayouts = { ...worktreeLayouts };
        for (const [wtPath, parkedLayout] of Object.entries(nextWorktreeLayouts)) {
          if (parkedLayout.tabs.some((t) => t.id === action.tabId)) {
            nextWorktreeLayouts[wtPath] = layoutReducer(parkedLayout, {
              type: "CLOSE_TAB",
              tabId: action.tabId,
              replacementTab: action.replacement?.tab,
            });
          }
        }
        worktreeLayouts = nextWorktreeLayouts;
      }

      const nextState = {
        ...state,
        sessions,
        activityBySessionId,
        unreadTabIds,
        layout,
        worktreeLayouts,
      };
      return clearWorktreeUnreadWhenRead(nextState, action.tabId, state);
    }
    case "ACTIVATE_TAB": {
      if (state.layout.tabs.some((tab) => tab.id === action.tabId)) {
        const unreadTabIds = { ...state.unreadTabIds };
        delete unreadTabIds[action.tabId];
        const nextState = acknowledgeTabCompletions(
          {
            ...state,
            unreadTabIds,
            layout: layoutReducer(state.layout, { type: "ACTIVATE_TAB", tabId: action.tabId }),
          },
          action.tabId,
        );
        return clearWorktreeUnreadWhenRead(nextState, action.tabId, state);
      }
      for (const [wtPath, parkedLayout] of Object.entries(state.worktreeLayouts ?? {})) {
        if (parkedLayout.tabs.some((tab) => tab.id === action.tabId)) {
          const nextWorktreeLayouts = { ...(state.worktreeLayouts ?? {}) };
          if (state.activeWorktreePath) {
            nextWorktreeLayouts[state.activeWorktreePath] = state.layout;
          }
          delete nextWorktreeLayouts[wtPath];
          const nextLayout = layoutReducer(parkedLayout, { type: "ACTIVATE_TAB", tabId: action.tabId });
          const unreadTabIds = { ...state.unreadTabIds };
          delete unreadTabIds[action.tabId];
          const unreadWorktreePaths = { ...state.unreadWorktreePaths };
          delete unreadWorktreePaths[wtPath];
          const nextState = acknowledgeTabCompletions(
            {
              ...state,
              activeWorktreePath: wtPath,
              layout: nextLayout,
              worktreeLayouts: nextWorktreeLayouts,
              unreadTabIds,
              unreadWorktreePaths,
            },
            action.tabId,
          );
          return clearWorktreeUnreadWhenRead(nextState, action.tabId, state);
        }
      }
      return state;
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
          return [
            id,
            {
              ...session,
              lifecycle: action.lifecycle,
              reconnectLifecycle: action.lifecycle === "exited" ? "idle" : session.reconnectLifecycle,
            },
          ];
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
    case "SET_RECONNECT_LIFECYCLE": {
      const session = state.sessions[action.sessionId];
      if (!session) return state;
      return {
        ...state,
        sessions: {
          ...state.sessions,
          [action.sessionId]: {
            ...session,
            reconnectLifecycle: action.lifecycle,
            reconnectError: action.error ?? null,
            reconnectRequestId: action.requestId ?? session.reconnectRequestId ?? null,
          },
        },
      };
    }
    case "APPLY_PROVIDER_SESSION_IF_MISSING": {
      const session = state.sessions[action.sessionId];
      if (!session) return state;
      const nextProviderSession = session.providerSession ?? action.providerSession;
      const nextAgentType = session.agentType ?? (action.agentType || null);
      if (nextProviderSession === session.providerSession && nextAgentType === session.agentType) {
        return state;
      }
      return {
        ...state,
        sessions: {
          ...state.sessions,
          [action.sessionId]: {
            ...session,
            providerSession: nextProviderSession,
            ...(nextAgentType ? { agentType: nextAgentType } : {}),
          },
        },
      };
    }
    case "REBIND_SESSION_BACKEND": {
      const session = state.sessions[action.sessionId];
      if (!session) return state;
      return {
        ...state,
        sessions: {
          ...state.sessions,
          [action.sessionId]: {
            ...session,
            backendSessionId: action.backendSessionId,
            cwd: action.cwd ?? session.cwd,
            daemonEpoch: action.daemonEpoch ?? null,
            lastOutputSequence: null,
            lifecycle: "running",
            reconnectLifecycle: "idle",
            reconnectError: null,
            reconnectRequestId: null,
          },
        },
      };
    }
    case "SESSION_SCREEN_ACTIVITY": {
      const previous = state.activityBySessionId?.[action.sessionId];
      let mappedState: TerminalActivityState;
      if (action.state === "working") {
        mappedState = "working";
      } else if (action.state === "blocked") {
        mappedState = "waiting";
      } else if (action.state === "idle") {
        if (!previous || (previous.state !== "working" && previous.state !== "waiting")) {
          return state;
        }
        mappedState = "done";
      } else {
        return state;
      }

      const authoritativeAgentType = state.sessions[action.sessionId]?.agentType ?? null;
      const normalizedManifestId = action.manifestId?.trim().toLowerCase();
      const isSupportedManifest = Boolean(normalizedManifestId && resolveAgentLogo(normalizedManifestId));
      const prevActivity = previous as WorkspaceTerminalActivity | undefined;
      const agentType = authoritativeAgentType
        ? authoritativeAgentType
        : isSupportedManifest
          ? normalizedManifestId
          : prevActivity?.agentType;
      const isAgent = authoritativeAgentType ? true : isSupportedManifest ? true : (prevActivity?.isAgent ?? false);
      const agentSource = authoritativeAgentType
        ? prevActivity?.agentSource
        : isSupportedManifest
          ? "screen"
          : prevActivity?.agentSource;

      const activity: WorkspaceTerminalActivity = {
        state: mappedState,
        title: previous?.title ?? "",
        isAgent,
        agentType,
        source: "screen",
        agentSource,
      };
      const nextState = applySessionActivity(state, action.tabId, action.sessionId, activity);
      const session = nextState.sessions[action.sessionId];
      if (!session) return nextState;

      const updatedAgentType = session.agentType ?? (agentType || null);
      const updatedProviderSession = action.providerSession ?? session.providerSession;

      if (updatedAgentType === session.agentType && updatedProviderSession === session.providerSession) {
        return nextState;
      }

      return {
        ...nextState,
        sessions: {
          ...nextState.sessions,
          [action.sessionId]: {
            ...session,
            ...(updatedAgentType ? { agentType: updatedAgentType } : {}),
            ...(updatedProviderSession ? { providerSession: updatedProviderSession } : {}),
          },
        },
      };
    }
    case "MARK_SESSION_ACTIVITY_SEEN": {
      const activity = state.activityBySessionId?.[action.sessionId];
      if (!activity || activity.state !== "done" || activity.seen) return state;
      return {
        ...state,
        activityBySessionId: {
          ...state.activityBySessionId,
          [action.sessionId]: { ...activity, seen: true },
        },
      };
    }
    case "SESSION_TITLE_ACTIVITY": {
      const previous = state.activityBySessionId?.[action.sessionId];
      const prevActivity = previous as WorkspaceTerminalActivity | undefined;
      const isScreenSource = previous?.source === "screen";
      const isScreenAgent = isScreenSource && prevActivity?.agentSource === "screen";

      if (isBareAgentTitle(action.title)) {
        if (isScreenSource) {
          const parsed = parseAgentTitle(action.title);
          const normalizedTitle = normalizeTerminalTitle(action.title);
          const isAgent = parsed?.isAgent ? true : (isScreenAgent ? (previous.isAgent ?? false) : false);
          const agentType = parsed?.isAgent ? parsed.agentType : (isScreenAgent ? previous.agentType : undefined);
          const agentSource = parsed?.isAgent ? "title" : (isScreenAgent ? "screen" : undefined);
          const activity: WorkspaceTerminalActivity = {
            state: previous.state,
            title: formatTabLabelFromTitle(action.title, normalizedTitle),
            isAgent,
            agentType,
            source: "screen",
            agentSource,
          };
          return applySessionActivity(state, action.tabId, action.sessionId, activity);
        }
        if (!previous) return state;
        const activityBySessionId = { ...(state.activityBySessionId ?? {}) };
        delete activityBySessionId[action.sessionId];
        return { ...state, activityBySessionId };
      }

      const parsed = parseAgentTitle(action.title);
      const classified = classifyTerminalTitleActivity(action.title);

      if (!classified && !previous) {
        return state;
      }

      // A shell prompt repainting the title (cwd, `zsh`, …) is not evidence that the agent
      // stopped: agents leave the title alone while a turn runs. Only drop the entry once the
      // run has actually settled, so an in-flight state survives the repaint the same way a
      // screen-derived one already does. The stale brand is still dropped below.
      const inFlight = previous?.state === "working" || previous?.state === "waiting";

      if (!classified && !parsed?.isAgent && !isScreenSource && !inFlight) {
        if (!previous) return state;
        const activityBySessionId = { ...(state.activityBySessionId ?? {}) };
        delete activityBySessionId[action.sessionId];
        return { ...state, activityBySessionId };
      }

      const normalizedTitle = normalizeTerminalTitle(action.title);
      const isAgent = parsed?.isAgent
        ? true
        : isScreenAgent && inFlight
          ? (previous?.isAgent ?? false)
          : false;
      const agentType = parsed?.isAgent
        ? parsed.agentType
        : isScreenAgent && inFlight
          ? previous?.agentType
          : undefined;
      const agentSource = parsed?.isAgent
        ? "title"
        : isScreenAgent && inFlight
          ? "screen"
          : undefined;

      const activity: WorkspaceTerminalActivity = {
        state: isScreenSource ? previous.state : (classified ?? previous!.state),
        title: formatTabLabelFromTitle(action.title, normalizedTitle),
        isAgent,
        agentType,
        source: isScreenSource ? "screen" : (previous?.source ?? "title"),
        agentSource,
      };
      return applySessionActivity(state, action.tabId, action.sessionId, activity);
    }
  }
}

/**
 * Marks every completion the given tab owns as seen.
 *
 * Opening a tab is how the user reads its completion, so the green dot must go out — the same way
 * activating a tab already clears its unread flag. The entries are kept so the tab retains its agent
 * brand icon; only the attention signal is consumed.
 */
function acknowledgeTabCompletions(state: WorkspaceState, tabId: string): WorkspaceState {
  const activityBySessionId = state.activityBySessionId ?? {};
  let changed = false;
  const next = { ...activityBySessionId };

  for (const sessionId of getTabSessionIds(state, tabId)) {
    const activity = activityBySessionId[sessionId];
    if (!activity || activity.state !== "done" || activity.seen) continue;
    next[sessionId] = { ...activity, seen: true };
    changed = true;
  }

  return changed ? { ...state, activityBySessionId: next } : state;
}

function isTabVisible(state: WorkspaceState, tabId: string): boolean {
  if (state.layout.activeTabId === tabId) return true;
  if (state.layout.tabGroups) {
    for (const group of Object.values(state.layout.tabGroups)) {
      if (group.activeTabId === tabId) return true;
    }
  }
  return false;
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
    previous.agentType === activity.agentType &&
    previous.source === activity.source &&
    previous.seen === activity.seen
  ) {
    return state;
  }

  // A completion the user is already looking at is not a request for attention, so it is born
  // acknowledged. This is what keeps an agent that boots with a spinner and settles at its prompt
  // from leaving a permanent dot on the tab in front of the user.
  const acknowledged =
    activity.state === "done" && (activity.seen === true || isTabVisible(state, tabId));
  const stored: TerminalActivity = acknowledged ? { ...activity, seen: true } : activity;

  let nextState: WorkspaceState = {
    ...state,
    activityBySessionId: { ...(state.activityBySessionId ?? {}), [sessionId]: stored },
  };

  if (activity.state === "done" && previous?.state !== "done" && !isTabVisible(state, tabId)) {
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

function getAllTabs(state: WorkspaceState): WorkspaceTab[] {
  return [...state.layout.tabs, ...Object.values(state.worktreeLayouts ?? {}).flatMap((l) => l.tabs)];
}

function getTabSessionIdsForLayout(layout: LayoutState, tabId: string): Set<string> {
  const tab = layout.tabs.find((candidate) => candidate.id === tabId);
  if (!tab || tab.kind === "browser") return new Set<string>();
  const tabLayout = layout.layoutsByTabId?.[tabId];
  const sessionIds = new Set<string>(Object.values(tabLayout?.sessionIdsByLeafId ?? {}));
  sessionIds.add(tab.sessionId);
  sessionIds.delete("");
  return sessionIds;
}

function getTabSessionIds(state: WorkspaceState, tabId: string): Set<string> {
  const inActive = getTabSessionIdsForLayout(state.layout, tabId);
  if (inActive.size > 0 || state.layout.tabs.some((t) => t.id === tabId)) return inActive;
  for (const layout of Object.values(state.worktreeLayouts ?? {})) {
    const inParked = getTabSessionIdsForLayout(layout, tabId);
    if (inParked.size > 0 || layout.tabs.some((t) => t.id === tabId)) return inParked;
  }
  return new Set<string>();
}

function isSessionReferencedOutsideTab(state: WorkspaceState, sessionId: string, tabId: string): boolean {
  return getAllTabs(state).some((tab) => tab.id !== tabId && getTabSessionIds(state, tab.id).has(sessionId));
}

function isSessionReferenced(state: WorkspaceState, sessionId: string): boolean {
  return getAllTabs(state).some((tab) => getTabSessionIds(state, tab.id).has(sessionId));
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
  for (const tab of getAllTabs(state)) {
    if (tab.id === tabId && tab.kind === "browser") {
      return tab.worktreePath ?? null;
    }
  }
  return null;
}

function findTabIdForSession(state: WorkspaceState, sessionId: string): string | null {
  for (const tab of getAllTabs(state)) {
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
  const allTabs = getAllTabs(sourceState);
  const hasOtherUnreadTab = allTabs.some(
    (tab) => tab.id !== tabId && state.unreadTabIds[tab.id] && getTabWorktreePath(sourceState, tab.id) === worktreePath,
  );
  if (hasOtherUnreadTab) return state;
  const unreadWorktreePaths = { ...state.unreadWorktreePaths };
  delete unreadWorktreePaths[worktreePath];
  return { ...state, unreadWorktreePaths };
}

async function spawnTerminalForLogicalAction(
  services: WorkspaceServices,
  request: { workspaceId: string; worktree: WorktreeIdentity | null; cwd?: string | null; shell?: string | null },
): Promise<string> {
  const clientRequestId = createClientRequestId();
  const stableRequest = { ...request, clientRequestId };
  try {
    return await services.spawnTerminal(stableRequest);
  } catch (error) {
    if (!isAmbiguousRendererTransportError(error)) throw error;
    return services.spawnTerminal(stableRequest);
  }
}

function isAmbiguousRendererTransportError(error: unknown): boolean {
  if (!error || typeof error !== "object") return true;
  const code = (error as { code?: unknown }).code;
  return code === undefined || code === "UNKNOWN";
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

function nextTabLabel(worktree: Worktree, allTabs: WorkspaceTab[], sessions: Record<string, TerminalSession>) {
  const branch = worktree.branch?.replace(/^refs\/heads\//, "") ?? "";
  const parts = branch.split("/");
  const base =
    parts[0] === "orca" && parts.length > 2
      ? parts.slice(2).join("/")
      : branch && branch !== "orca-lite"
        ? branch
        : "main";
  const count = allTabs.filter(
    (tab) => tab.kind !== "browser" && sessionWorktreePath(sessions[tab.sessionId]) === worktree.path,
  ).length + 1;
  return count === 1 ? base : `${base} (${count})`;
}

function mapBackendLifecycle(payload: TerminalLifecyclePayload): TerminalLifecycle {
  if (payload.state === "started") return "working";
  if (payload.state === "failed") return "failed";
  return "exited";
}

function createClientRequestId() {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function createId(prefix: string) {
  const randomPart = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return `${prefix}:${randomPart}`;
}
