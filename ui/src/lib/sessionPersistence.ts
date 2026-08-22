import { normalizeLayout } from "../state/layout";
import { collectLeafIds, createLeafNode } from "../state/paneTree";
import type { WorkspaceState } from "../state/workspaceStore";
import {
  worktreeIdentity,
  type BrowserTab,
  type LayoutState,
  type PersistedLayout,
  type PersistedTab,
  type PersistedTerminalSession,
  type PersistedWorkspace,
  type PersistedWorkspaceSession,
  type PersistedWorktree,
  type TabGroup,
  type TerminalSession,
  type TerminalTab,
  type WorkspaceTab,
  type Worktree,
} from "./types";

export const WORKSPACE_SESSION_VERSION = 2;

export function serializeWorkspaceState(
  workspaceId: string,
  repoRoot: string,
  state: WorkspaceState,
): PersistedWorkspaceSession {
  const normalizedLayout = normalizeLayout(state.layout);
  const persistedWorktrees: PersistedWorktree[] = state.worktrees.map((wt) => ({
    path: wt.path,
    branch: wt.branch ?? "",
    head: wt.head,
    isMain: !wt.branch || wt.branch === "main" || wt.branch === "master",
    isLocked: Boolean(wt.locked),
  }));

  const persistedTabs: PersistedTab[] = normalizedLayout.tabs.map((tab) => {
    if (tab.kind === "browser") {
      return {
        id: tab.id,
        kind: "browser",
        label: tab.label,
        pinned: Boolean(tab.pinned),
        browser: {
          browserId: tab.browserId,
          url: tab.url,
          title: tab.title ?? null,
          loading: tab.loading,
          canGoBack: tab.canGoBack,
          canGoForward: tab.canGoForward,
        },
      };
    }

    const tabLayout = normalizedLayout.layoutsByTabId[tab.id];
    return {
      id: tab.id,
      kind: "terminal",
      label: tab.label,
      pinned: Boolean(tab.pinned),
      terminal: tabLayout
        ? {
            primarySessionId: tab.sessionId,
            paneTree: tabLayout.root,
            sessionIdsByLeafId: { ...tabLayout.sessionIdsByLeafId },
            activeLeafId: tabLayout.activeLeafId,
            expandedLeafId: tabLayout.expandedLeafId,
          }
        : {
            primarySessionId: tab.sessionId,
            paneTree: createLeafNode(`leaf-persisted:${tab.id}`),
            sessionIdsByLeafId: { [`leaf-persisted:${tab.id}`]: tab.sessionId },
            activeLeafId: `leaf-persisted:${tab.id}`,
            expandedLeafId: null,
          },
    };
  });

  const persistedLayout: PersistedLayout = {
    splitMode: normalizedLayout.split ?? "none",
    primaryTabId: normalizedLayout.primaryTabId ?? null,
    secondaryTabId: normalizedLayout.secondaryTabId ?? null,
    activeTabId: normalizedLayout.activeTabId ?? null,
    tabs: persistedTabs,
    tabGroups: Object.values(normalizedLayout.tabGroups ?? {}).map((group) => ({
      id: group.id,
      tabIds: [...group.tabIds],
      activeTabId: group.activeTabId,
    })),
    tabGroupLayout: normalizedLayout.tabGroupLayout ?? null,
    focusedGroupId: normalizedLayout.focusedGroupId ?? null,
  };

  const persistedTerminalSessions: Record<string, PersistedTerminalSession> = {};
  const createdAt = Date.now();
  for (const [id, sess] of Object.entries(state.sessions)) {
    if (!sess) continue;
    persistedTerminalSessions[id] = {
      localSessionId: sess.id,
      backendSessionId: sess.backendSessionId,
      worktreePath: sess.worktreePath ?? sess.cwd,
      cwd: sess.cwd,
      createdAt,
    };
  }

  const workspace: PersistedWorkspace = {
    workspaceId,
    repoRoot,
    worktrees: persistedWorktrees,
    activeWorktreePath: state.activeWorktreePath,
    layout: persistedLayout,
    terminalSessions: persistedTerminalSessions,
  };

  return {
    version: WORKSPACE_SESSION_VERSION,
    timestamp: Date.now(),
    activeWorkspaceId: workspaceId,
    workspaces: {
      [workspaceId]: workspace,
    },
  };
}

export function deserializeWorkspaceState(
  workspaceId: string,
  persistedSession: PersistedWorkspaceSession,
  liveBackendSessionIds?: Iterable<string> | null,
): WorkspaceState | null {
  const ws = persistedSession.workspaces?.[workspaceId];
  if (!ws) return null;
  const isV2 = (persistedSession.version ?? 1) >= 2;

  const liveSet = liveBackendSessionIds
    ? (liveBackendSessionIds instanceof Set ? liveBackendSessionIds : new Set(liveBackendSessionIds))
    : null;

  const worktrees: Worktree[] = (ws.worktrees || []).map((wt) => ({
    path: wt.path,
    branch: wt.branch ? (wt.branch.startsWith("refs/heads/") ? wt.branch : `refs/heads/${wt.branch}`) : null,
    head: wt.head,
    bare: false,
    detached: false,
    locked: wt.isLocked ? "locked" : null,
    prunable: null,
  }));

  const sessions: Record<string, TerminalSession> = {};
  for (const [mapKey, sess] of Object.entries(ws.terminalSessions || {})) {
    const localSessionId = sess.localSessionId || mapKey || sess.sessionId || "";
    if (!localSessionId) continue;
    const persistedBackendSessionId = isV2 ? (sess.backendSessionId ?? null) : (sess.backendSessionId ?? sess.sessionId ?? null);
    const backendSessionId = liveSet
      ? (persistedBackendSessionId && liveSet.has(persistedBackendSessionId) ? persistedBackendSessionId : null)
      : persistedBackendSessionId;
    const worktreePath = sess.worktreePath || sess.cwd;
    const matchingWorktree = worktrees.find((wt) => wt.path === worktreePath);
    sessions[localSessionId] = {
      id: localSessionId,
      cwd: sess.cwd || worktreePath,
      worktreePath,
      workspaceId,
      worktree: matchingWorktree ? worktreeIdentity(matchingWorktree) : null,
      backendSessionId,
      lifecycle: backendSessionId ? "working" : "exited",
    };
  }

  const tabs: WorkspaceTab[] = (ws.layout?.tabs || []).map((persistedTab): WorkspaceTab => {
    if (persistedTab.kind === "browser" || persistedTab.browser) {
      const browser = persistedTab.browser;
      const tab: BrowserTab = {
        id: persistedTab.id,
        kind: "browser",
        label: persistedTab.label,
        browserId: browser?.browserId ?? persistedTab.sessionId ?? `restored-browser:${persistedTab.id}`,
        url: browser?.url ?? "about:blank",
        title: browser?.title ?? persistedTab.label,
        canGoBack: browser?.canGoBack ?? false,
        canGoForward: browser?.canGoForward ?? false,
        loading: browser?.loading ?? false,
        pinned: Boolean(persistedTab.pinned),
      };
      return tab;
    }

    const primarySessionId = persistedTab.terminal?.primarySessionId ?? persistedTab.sessionId ?? "";
    const tab: TerminalTab = {
      id: persistedTab.id,
      kind: "terminal",
      sessionId: primarySessionId,
      label: persistedTab.label,
      pinned: Boolean(persistedTab.pinned),
    };
    return tab;
  });

  const layoutsByTabId: LayoutState["layoutsByTabId"] = {};
  for (const persistedTab of ws.layout?.tabs || []) {
    if (persistedTab.kind === "browser" || persistedTab.browser) {
      const leafId = `leaf-browser:${persistedTab.id}`;
      layoutsByTabId[persistedTab.id] = {
        root: createLeafNode(leafId),
        activeLeafId: leafId,
        expandedLeafId: null,
        sessionIdsByLeafId: { [leafId]: "" },
      };
      continue;
    }

    const terminal = persistedTab.terminal;
    const paneTree = terminal?.paneTree ?? persistedTab.paneTree;
    const primarySessionId = terminal?.primarySessionId ?? persistedTab.sessionId ?? "";
    const root = paneTree ?? createLeafNode(`leaf-restored:${persistedTab.id}`);
    const leafIds = collectLeafIds(root);
    const persistedMapping = terminal?.sessionIdsByLeafId ?? persistedTab.sessionIdsByLeafId;
    const sessionIdsByLeafId = persistedMapping && Object.keys(persistedMapping).length > 0
      ? Object.fromEntries(leafIds.map((leafId) => [leafId, persistedMapping[leafId] ?? primarySessionId]))
      : Object.fromEntries(leafIds.map((leafId) => [leafId, primarySessionId]));
    const requestedActiveLeafId = terminal?.activeLeafId ?? persistedTab.activeLeafId ?? null;
    const requestedExpandedLeafId = terminal?.expandedLeafId ?? persistedTab.expandedLeafId ?? null;
    layoutsByTabId[persistedTab.id] = {
      root,
      activeLeafId: requestedActiveLeafId && leafIds.includes(requestedActiveLeafId) ? requestedActiveLeafId : leafIds[0] ?? null,
      expandedLeafId:
        requestedExpandedLeafId && leafIds.includes(requestedExpandedLeafId) ? requestedExpandedLeafId : null,
      sessionIdsByLeafId,
    };
  }

  const tabGroups: Record<string, TabGroup> | undefined = ws.layout?.tabGroups?.length
    ? Object.fromEntries(
        ws.layout.tabGroups.map((group) => [
          group.id,
          { id: group.id, tabIds: [...group.tabIds], activeTabId: group.activeTabId },
        ]),
      )
    : undefined;

  const layout = normalizeLayout({
    tabs,
    primaryTabId: ws.layout?.primaryTabId || (tabs[0]?.id ?? null),
    secondaryTabId: ws.layout?.secondaryTabId || null,
    activeTabId: ws.layout?.activeTabId || (tabs[0]?.id ?? null),
    split: (ws.layout?.splitMode as LayoutState["split"]) || "none",
    nestedSplit: null,
    layoutsByTabId,
    tabGroups,
    tabGroupLayout: ws.layout?.tabGroupLayout ?? null,
    focusedGroupId: ws.layout?.focusedGroupId ?? null,
  });

  return {
    worktrees,
    activeWorktreePath: ws.activeWorktreePath || (worktrees[0]?.path ?? null),
    sessions,
    layout,
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  };
}
