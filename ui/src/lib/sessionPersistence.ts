import { createLayoutState, normalizeLayout } from "../state/layout";
import { collectLeafIds, createLeafNode, removeLeaf, type PaneNode } from "../state/paneTree";
import type { WorkspaceState } from "../state/workspaceStore";
import type { TerminalActivity } from "./activity";
import { loadBrowserSettings, resolveSupportedBrowserProfileId, supportedBrowserProfiles } from "./browserSettings";
import {
  createBrowserPaneContent,
  createDagPaneContent,
  createTerminalPaneContent,
  worktreeIdentity,
  type BrowserPaneState,
  type BrowserTab,
  type LayoutState,
  type PaneContent,
  type PersistedLayout,
  type PersistedTab,
  type PersistedTerminalSession,
  type PersistedWorkspace,
  type PersistedWorkspaceSession,
  type PersistedWorktree,
  type TabGroup,
  type TerminalLifecycle,
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
  existingSession?: PersistedWorkspaceSession | null,
): PersistedWorkspaceSession {
  const browserSettings = loadBrowserSettings();
  const restoreBrowserTabs = browserSettings.restoreTabsOnLaunch;

  const persistedWorktrees: PersistedWorktree[] = state.worktrees.map((wt) => ({
    path: wt.path,
    branch: wt.branch ?? "",
    head: wt.head,
    isMain: !wt.branch || wt.branch === "main" || wt.branch === "master",
    isLocked: Boolean(wt.locked),
  }));

  function serializeLayout(layoutState: LayoutState): PersistedLayout {
    const normalizedLayout = normalizeLayout(layoutState);
    const persistedTabs: PersistedTab[] = [];
    for (const tab of normalizedLayout.tabs) {
      if (tab.kind === "browser") {
        if (!restoreBrowserTabs) continue;
        persistedTabs.push({
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
            zoomFactor: tab.zoomFactor,
            profileId: tab.profileId,
            worktreePath: tab.worktreePath,
            worktreeLabel: tab.worktreeLabel,
          },
        });
        continue;
      }

      const tabLayout = normalizedLayout.layoutsByTabId[tab.id];
      let effectiveRoot: PaneNode | null = tabLayout?.root ?? createLeafNode(`leaf-persisted:${tab.id}`);
      let effectiveContents = tabLayout?.contentsByLeafId;

      if (!restoreBrowserTabs && tabLayout && effectiveContents) {
        for (const [leafId, content] of Object.entries(effectiveContents)) {
          if (content.kind === "browser") {
            const nextRoot = removeLeaf(effectiveRoot, leafId);
            if (!nextRoot) {
              effectiveRoot = null;
              break;
            }
            effectiveRoot = nextRoot;
          }
        }
        if (!effectiveRoot) continue;
      }

      const leafIds = collectLeafIds(effectiveRoot);
      const contentsByLeafId: Record<string, PaneContent> = {};
      const sessionIdsByLeafId: Record<string, string> = {};

      for (const leafId of leafIds) {
        const content = effectiveContents?.[leafId];
        if (content) {
          if (content.kind === "browser") {
            const rawBrowser = content.browser ?? content;
            contentsByLeafId[leafId] = createBrowserPaneContent({
              browserId: rawBrowser.browserId ?? "",
              url: rawBrowser.url ?? "",
              title: rawBrowser.title ?? null,
              loading: rawBrowser.loading ?? false,
              canGoBack: rawBrowser.canGoBack ?? false,
              canGoForward: rawBrowser.canGoForward ?? false,
              zoomFactor: rawBrowser.zoomFactor,
              profileId: rawBrowser.profileId,
              worktreePath: rawBrowser.worktreePath,
              worktreeLabel: rawBrowser.worktreeLabel,
            });
            sessionIdsByLeafId[leafId] = "";
          } else if (content.kind === "dag") {
            const rawDag = content.dag ?? content;
            contentsByLeafId[leafId] = createDagPaneContent({
              runId: rawDag.runId ?? null,
            });
            sessionIdsByLeafId[leafId] = "";
          } else {
            contentsByLeafId[leafId] = createTerminalPaneContent(content.sessionId);
            sessionIdsByLeafId[leafId] = content.sessionId;
          }
        } else {
          const sessId = tabLayout?.sessionIdsByLeafId?.[leafId] ?? tab.sessionId;
          contentsByLeafId[leafId] = createTerminalPaneContent(sessId);
          sessionIdsByLeafId[leafId] = sessId;
        }
      }

      const activeLeafId =
        tabLayout?.activeLeafId && leafIds.includes(tabLayout.activeLeafId)
          ? tabLayout.activeLeafId
          : leafIds[0] ?? null;
      const expandedLeafId =
        tabLayout?.expandedLeafId && leafIds.includes(tabLayout.expandedLeafId)
          ? tabLayout.expandedLeafId
          : null;

      persistedTabs.push({
        id: tab.id,
        kind: "terminal",
        label: tab.label,
        pinned: Boolean(tab.pinned),
        terminal: {
          primarySessionId: tab.sessionId,
          paneTree: effectiveRoot,
          sessionIdsByLeafId,
          contentsByLeafId,
          activeLeafId,
          expandedLeafId,
        },
      });
    }

    const tabIds = new Set(persistedTabs.map((t) => t.id));
    const activeTabId =
      normalizedLayout.activeTabId && tabIds.has(normalizedLayout.activeTabId)
        ? normalizedLayout.activeTabId
        : (persistedTabs[0]?.id ?? null);
    const primaryTabId =
      normalizedLayout.primaryTabId && tabIds.has(normalizedLayout.primaryTabId)
        ? normalizedLayout.primaryTabId
        : (persistedTabs[0]?.id ?? null);

    return {
      splitMode: normalizedLayout.split ?? "none",
      primaryTabId,
      secondaryTabId: normalizedLayout.secondaryTabId ?? null,
      activeTabId,
      tabs: persistedTabs,
      tabGroups: Object.values(normalizedLayout.tabGroups ?? {}).map((group) => ({
        id: group.id,
        tabIds: group.tabIds.filter((id) => tabIds.has(id)),
        activeTabId: group.activeTabId && tabIds.has(group.activeTabId) ? group.activeTabId : null,
      })),
      tabGroupLayout: normalizedLayout.tabGroupLayout ?? null,
      focusedGroupId: normalizedLayout.focusedGroupId ?? null,
    };
  }

  const persistedLayout = serializeLayout(state.layout);
  const persistedWorktreeLayouts: Record<string, PersistedLayout> = {};
  for (const [wtPath, layout] of Object.entries(state.worktreeLayouts ?? {})) {
    if (layout) {
      persistedWorktreeLayouts[wtPath] = serializeLayout(layout);
    }
  }

  const allPersistedLayouts = [persistedLayout, ...Object.values(persistedWorktreeLayouts)];
  const referencedSessionIds = new Set<string>();
  for (const layout of allPersistedLayouts) {
    for (const tab of layout.tabs) {
      if (tab.kind === "browser") continue;
      const terminal = tab.terminal;
      if (terminal?.contentsByLeafId) {
        for (const content of Object.values(terminal.contentsByLeafId)) {
          if (content && content.kind === "terminal" && content.sessionId) {
            referencedSessionIds.add(content.sessionId);
          }
        }
      } else {
        if (terminal?.primarySessionId) referencedSessionIds.add(terminal.primarySessionId);
        for (const sessionId of Object.values(terminal?.sessionIdsByLeafId ?? {})) {
          if (sessionId) referencedSessionIds.add(sessionId);
        }
      }
    }
  }

  const persistedTerminalSessions: Record<string, PersistedTerminalSession> = {};
  const createdAt = Date.now();
  for (const [id, sess] of Object.entries(state.sessions)) {
    if (!sess || !referencedSessionIds.has(id)) continue;
    persistedTerminalSessions[id] = {
      localSessionId: sess.id,
      backendSessionId: sess.backendSessionId,
      worktreePath: sess.worktreePath ?? sess.cwd,
      cwd: sess.cwd,
      daemonEpoch: sess.daemonEpoch != null ? String(sess.daemonEpoch) : null,
      lastOutputSequence: sess.lastOutputSequence != null ? String(sess.lastOutputSequence) : null,
      agentType: sess.agentType ?? null,
      agentSessionId: sess.agentSessionId ?? null,
      createdAt,
    };
  }

  const persistedActivity: Record<string, TerminalActivity> = {};
  if (state.activityBySessionId) {
    for (const [sessionId, activity] of Object.entries(state.activityBySessionId)) {
      if (activity && referencedSessionIds.has(sessionId)) {
        persistedActivity[sessionId] = { ...activity };
      }
    }
  }

  const workspace: PersistedWorkspace = {
    workspaceId,
    repoRoot,
    worktrees: persistedWorktrees,
    activeWorktreePath: state.activeWorktreePath,
    layout: persistedLayout,
    ...(Object.keys(persistedWorktreeLayouts).length > 0 ? { worktreeLayouts: persistedWorktreeLayouts } : {}),
    terminalSessions: persistedTerminalSessions,
    ...(Object.keys(persistedActivity).length > 0 ? { activityBySessionId: persistedActivity } : {}),
  };

  const workspaces = {
    ...(existingSession?.workspaces ?? {}),
    [workspaceId]: workspace,
  };

  return {
    version: WORKSPACE_SESSION_VERSION,
    timestamp: Date.now(),
    activeWorkspaceId: workspaceId,
    workspaces,
  };
}

export function deserializeWorkspaceState(
  workspaceId: string,
  persistedSession: PersistedWorkspaceSession,
  liveBackendSessionIds?:
    | Iterable<string | { sessionId: string; daemonEpoch?: string | null; worktreePath?: string | null }>
    | {
        epoch?: string | null;
        daemonEpoch?: string | null;
        sessionIds?: Iterable<string>;
        sessions?: Iterable<string | { sessionId: string; daemonEpoch?: string | null }>;
      }
    | null,
): WorkspaceState | null {
  const ws = persistedSession.workspaces?.[workspaceId];
  if (!ws) return null;
  const isV2 = (persistedSession.version ?? 1) >= 2;

  const browserSettings = loadBrowserSettings();
  const restoreBrowser = browserSettings.restoreTabsOnLaunch;
  const supportedProfiles = supportedBrowserProfiles(browserSettings);
  const knownProfiles = new Set(supportedProfiles.map((profile) => profile.id));
  const fallbackProfileId = resolveSupportedBrowserProfileId(browserSettings.defaultProfileId, browserSettings);

  let globalLiveEpoch: string | null = null;
  const liveSessionMap = new Map<string, { daemonEpoch: string | null }>();
  const hasLiveSessionQuery = liveBackendSessionIds !== null && liveBackendSessionIds !== undefined;

  if (hasLiveSessionQuery && liveBackendSessionIds) {
    if (
      typeof liveBackendSessionIds === "object" &&
      !("length" in liveBackendSessionIds) &&
      !liveBackendSessionIds[Symbol.iterator as keyof typeof liveBackendSessionIds]
    ) {
      const container = liveBackendSessionIds as {
        epoch?: string | null;
        daemonEpoch?: string | null;
        sessionIds?: Iterable<string>;
        sessions?: Iterable<string | { sessionId: string; daemonEpoch?: string | null }>;
      };
      const rawEpoch = container.epoch ?? container.daemonEpoch;
      if (rawEpoch != null) {
        globalLiveEpoch = String(rawEpoch);
      }
      const sessionList = container.sessions ?? container.sessionIds ?? [];
      for (const item of sessionList) {
        if (typeof item === "string") {
          liveSessionMap.set(item, { daemonEpoch: globalLiveEpoch });
        } else if (item && typeof item === "object" && "sessionId" in item) {
          const itemEpoch = item.daemonEpoch != null ? String(item.daemonEpoch) : globalLiveEpoch;
          liveSessionMap.set(item.sessionId, { daemonEpoch: itemEpoch });
        }
      }
    } else {
      for (const item of liveBackendSessionIds as Iterable<any>) {
        if (typeof item === "string") {
          liveSessionMap.set(item, { daemonEpoch: null });
        } else if (item && typeof item === "object" && "sessionId" in item) {
          const itemEpoch = item.daemonEpoch != null ? String(item.daemonEpoch) : null;
          if (itemEpoch !== null && globalLiveEpoch === null) {
            globalLiveEpoch = itemEpoch;
          }
          liveSessionMap.set(item.sessionId, { daemonEpoch: itemEpoch });
        }
      }
    }
  }

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
    const persistedEpoch = sess.daemonEpoch != null ? String(sess.daemonEpoch) : null;
    const persistedSequence = sess.lastOutputSequence != null ? String(sess.lastOutputSequence) : null;

    let backendSessionId: string | null = null;
    let daemonEpoch: string | null = null;
    let lastOutputSequence: string | null = null;
    let lifecycle: TerminalLifecycle = "exited";

    if (!hasLiveSessionQuery) {
      const isLive = Boolean(persistedBackendSessionId);
      backendSessionId = isLive ? persistedBackendSessionId : null;
      daemonEpoch = isLive ? persistedEpoch : null;
      lastOutputSequence = isLive ? persistedSequence : null;
      lifecycle = isLive ? "working" : "exited";
    } else {
      if (!persistedBackendSessionId || !liveSessionMap.has(persistedBackendSessionId)) {
        backendSessionId = null;
        daemonEpoch = null;
        lastOutputSequence = null;
        lifecycle = "exited";
      } else {
        const liveInfo = liveSessionMap.get(persistedBackendSessionId);
        const effectiveLiveEpoch = liveInfo?.daemonEpoch ?? globalLiveEpoch;

        // A hit in liveSessionMap came from listSessions on the daemon running right now, and
        // backend ids do not survive a daemon restart. So a live hit already proves the PTY is
        // ours; only a RECORDED epoch that disagrees can disprove it.
        //
        // Treating a missing persisted epoch as a mismatch orphaned every restored session,
        // because no reducer writes daemonEpoch onto a session -- it is null in every save file.
        let epochMatches = true;
        if (effectiveLiveEpoch !== null && persistedEpoch !== null) {
          epochMatches = effectiveLiveEpoch === persistedEpoch;
        } else if (liveInfo === undefined && effectiveLiveEpoch !== null && persistedEpoch === null) {
          epochMatches = false;
        }

        if (epochMatches) {
          backendSessionId = persistedBackendSessionId;
          daemonEpoch = persistedEpoch ?? effectiveLiveEpoch ?? null;
          lastOutputSequence = persistedSequence;
          lifecycle = "working";
        } else {
          backendSessionId = null;
          daemonEpoch = null;
          lastOutputSequence = null;
          lifecycle = "exited";
        }
      }
    }

    const worktreePath = sess.worktreePath || sess.cwd;
    const matchingWorktree = worktrees.find((wt) => wt.path === worktreePath);
    const agentType = sess.agentType ?? null;
    const agentSessionId = sess.agentSessionId ?? null;
    sessions[localSessionId] = {
      id: localSessionId,
      cwd: sess.cwd || worktreePath,
      worktreePath,
      workspaceId,
      worktree: matchingWorktree ? worktreeIdentity(matchingWorktree) : null,
      backendSessionId,
      lifecycle,
      daemonEpoch,
      lastOutputSequence,
      agentType,
      agentSessionId,
    };
  }

  function deserializeLayout(persistedLayout: PersistedLayout | undefined): LayoutState {
    if (!persistedLayout) return createLayoutState();

    const tabs: WorkspaceTab[] = [];
    for (const persistedTab of persistedLayout.tabs || []) {
      if (persistedTab.kind === "browser" || persistedTab.browser) {
        if (!restoreBrowser) continue;
        const browser = persistedTab.browser;
        const rawProfileId = browser?.profileId;
        const profileId = rawProfileId && knownProfiles.has(rawProfileId) ? rawProfileId : fallbackProfileId;
        const tab: BrowserTab = {
          id: persistedTab.id,
          kind: "browser",
          label: persistedTab.label,
          browserId: browser?.browserId ?? persistedTab.sessionId ?? `restored-browser:${persistedTab.id}`,
          url: browser?.url ?? "about:blank",
          title: browser?.title ?? persistedTab.label,
          canGoBack: browser?.canGoBack ?? false,
          canGoForward: browser?.canGoForward ?? false,
          zoomFactor: browser?.zoomFactor,
          loading: browser?.loading ?? false,
          pinned: Boolean(persistedTab.pinned),
          profileId,
          worktreePath: browser?.worktreePath ?? persistedTab.worktreePath,
          worktreeLabel: browser?.worktreeLabel,
        };
        tabs.push(tab);
        continue;
      }

      const primarySessionId = persistedTab.terminal?.primarySessionId ?? persistedTab.sessionId ?? "";
      const tab: TerminalTab = {
        id: persistedTab.id,
        kind: "terminal",
        sessionId: primarySessionId,
        label: persistedTab.label,
        pinned: Boolean(persistedTab.pinned),
      };
      tabs.push(tab);
    }

    const layoutsByTabId: LayoutState["layoutsByTabId"] = {};
    for (const persistedTab of persistedLayout.tabs || []) {
      if (persistedTab.kind === "browser" || persistedTab.browser) {
        if (!restoreBrowser) continue;
        const browser = persistedTab.browser;
        const rawProfileId = browser?.profileId;
        const profileId = rawProfileId && knownProfiles.has(rawProfileId) ? rawProfileId : fallbackProfileId;
        const leafId = `leaf-browser:${persistedTab.id}`;
        const browserState: BrowserPaneState = {
          browserId: browser?.browserId ?? persistedTab.sessionId ?? `restored-browser:${persistedTab.id}`,
          url: browser?.url ?? "about:blank",
          title: browser?.title ?? persistedTab.label,
          canGoBack: browser?.canGoBack ?? false,
          canGoForward: browser?.canGoForward ?? false,
          zoomFactor: browser?.zoomFactor,
          loading: browser?.loading ?? false,
          profileId,
          worktreePath: browser?.worktreePath ?? persistedTab.worktreePath,
          worktreeLabel: browser?.worktreeLabel,
        };
        layoutsByTabId[persistedTab.id] = {
          root: createLeafNode(leafId),
          activeLeafId: leafId,
          expandedLeafId: null,
          sessionIdsByLeafId: { [leafId]: "" },
          contentsByLeafId: { [leafId]: createBrowserPaneContent(browserState) },
        };
        continue;
      }

      const terminal = persistedTab.terminal;
      const legacyTabLayout = persistedLayout.layoutsByTabId?.[persistedTab.id];
      const paneTree = terminal?.paneTree ?? persistedTab.paneTree ?? legacyTabLayout?.root;
      const primarySessionId = terminal?.primarySessionId ?? persistedTab.sessionId ?? "";
      let root: PaneNode | null = paneTree ?? createLeafNode(`leaf-restored:${persistedTab.id}`);
      const persistedContents = terminal?.contentsByLeafId ?? persistedTab.contentsByLeafId;
      const persistedMapping =
        terminal?.sessionIdsByLeafId ?? persistedTab.sessionIdsByLeafId ?? legacyTabLayout?.sessionIdsByLeafId;

      if (!restoreBrowser && persistedContents) {
        for (const [leafId, content] of Object.entries(persistedContents)) {
          if (content && content.kind === "browser") {
            const nextRoot = removeLeaf(root, leafId);
            if (!nextRoot) {
              root = null;
              break;
            }
            root = nextRoot;
          }
        }
        if (!root) continue;
      }

      const leafIds = collectLeafIds(root);
      const contentsByLeafId: Record<string, PaneContent> = {};
      const sessionIdsByLeafId: Record<string, string> = {};

      for (const leafId of leafIds) {
        const rawContent = persistedContents?.[leafId];
        if (rawContent) {
          if (rawContent.kind === "browser") {
            const rawBrowser = rawContent.browser ?? rawContent;
            const rawProfileId = rawBrowser.profileId;
            const profileId = rawProfileId && knownProfiles.has(rawProfileId) ? rawProfileId : fallbackProfileId;
            contentsByLeafId[leafId] = createBrowserPaneContent({
              browserId: rawBrowser.browserId || `restored-browser:${persistedTab.id}:${leafId}`,
              url: rawBrowser.url || "about:blank",
              title: rawBrowser.title ?? null,
              loading: Boolean(rawBrowser.loading),
              canGoBack: Boolean(rawBrowser.canGoBack),
              canGoForward: Boolean(rawBrowser.canGoForward),
              zoomFactor: typeof rawBrowser.zoomFactor === "number" ? rawBrowser.zoomFactor : undefined,
              profileId,
              worktreePath: rawBrowser.worktreePath,
              worktreeLabel: rawBrowser.worktreeLabel,
            });
            sessionIdsByLeafId[leafId] = "";
          } else if (rawContent.kind === "dag") {
            const rawDag = rawContent.dag ?? rawContent;
            contentsByLeafId[leafId] = createDagPaneContent({
              runId: rawDag.runId ?? null,
            });
            sessionIdsByLeafId[leafId] = "";
          } else {
            const sessId = rawContent.sessionId || persistedMapping?.[leafId] || primarySessionId;
            contentsByLeafId[leafId] = createTerminalPaneContent(sessId);
            sessionIdsByLeafId[leafId] = sessId;
          }
        } else {
          const sessId = persistedMapping?.[leafId] || primarySessionId;
          contentsByLeafId[leafId] = createTerminalPaneContent(sessId);
          sessionIdsByLeafId[leafId] = sessId;
        }
      }

      const requestedActiveLeafId =
        terminal?.activeLeafId ?? persistedTab.activeLeafId ?? legacyTabLayout?.activeLeafId ?? null;
      const requestedExpandedLeafId =
        terminal?.expandedLeafId ?? persistedTab.expandedLeafId ?? legacyTabLayout?.expandedLeafId ?? null;
      layoutsByTabId[persistedTab.id] = {
        root,
        activeLeafId: requestedActiveLeafId && leafIds.includes(requestedActiveLeafId) ? requestedActiveLeafId : leafIds[0] ?? null,
        expandedLeafId:
          requestedExpandedLeafId && leafIds.includes(requestedExpandedLeafId) ? requestedExpandedLeafId : null,
        sessionIdsByLeafId,
        contentsByLeafId,
      };
    }

    const tabGroups: Record<string, TabGroup> | undefined = persistedLayout.tabGroups?.length
      ? Object.fromEntries(
          persistedLayout.tabGroups.map((group) => [
            group.id,
            { id: group.id, tabIds: [...group.tabIds], activeTabId: group.activeTabId },
          ]),
        )
      : undefined;

    return normalizeLayout({
      tabs,
      primaryTabId: persistedLayout.primaryTabId || (tabs[0]?.id ?? null),
      secondaryTabId: persistedLayout.secondaryTabId || null,
      activeTabId: persistedLayout.activeTabId || (tabs[0]?.id ?? null),
      split: (persistedLayout.splitMode as LayoutState["split"]) || "none",
      nestedSplit: null,
      layoutsByTabId,
      tabGroups,
      tabGroupLayout: persistedLayout.tabGroupLayout ?? null,
      focusedGroupId: persistedLayout.focusedGroupId ?? null,
    });
  }

  const activeLayout = deserializeLayout(ws.layout);
  const worktreeLayouts: Record<string, LayoutState> = {};
  if (ws.worktreeLayouts) {
    for (const [wtPath, persistedWtLayout] of Object.entries(ws.worktreeLayouts)) {
      if (persistedWtLayout) {
        worktreeLayouts[wtPath] = deserializeLayout(persistedWtLayout);
      }
    }
  }

  const allLayouts = [activeLayout, ...Object.values(worktreeLayouts)];
  const referencedSessionIds = new Set<string>();
  for (const layout of allLayouts) {
    for (const tab of layout.tabs) {
      if (tab.kind === "browser") continue;
      const tabLayout = layout.layoutsByTabId[tab.id];
      if (tabLayout?.contentsByLeafId) {
        for (const content of Object.values(tabLayout.contentsByLeafId)) {
          if (content && content.kind === "terminal" && content.sessionId) {
            referencedSessionIds.add(content.sessionId);
          }
        }
      } else {
        if (tab.sessionId) referencedSessionIds.add(tab.sessionId);
        for (const sessionId of Object.values(tabLayout?.sessionIdsByLeafId ?? {})) {
          if (sessionId) referencedSessionIds.add(sessionId);
        }
      }
    }
  }

  const referencedSessions = Object.fromEntries(
    Object.entries(sessions).filter(([sessionId]) => referencedSessionIds.has(sessionId)),
  );

  const restoredActivity: Record<string, TerminalActivity> = {};
  if (ws.activityBySessionId) {
    for (const [sessionId, activity] of Object.entries(ws.activityBySessionId)) {
      if (activity && referencedSessionIds.has(sessionId)) {
        // `working` and `waiting` are claims about a process that is alive right now. Nothing is
        // running yet after a restart, so carrying them over would show a spinner for an agent that
        // no longer exists. The agent identity is kept, so the tab still renders its icon.
        const isInFlightClaim = activity.state === "working" || activity.state === "waiting";
        restoredActivity[sessionId] = {
          state: isInFlightClaim ? "done" : activity.state,
          title: activity.title || "",
          isAgent: Boolean(activity.isAgent),
          ...(activity.agentType ? { agentType: activity.agentType } : {}),
          ...(activity.source ? { source: activity.source } : {}),
        };
      }
    }
  }

  return {
    workspaceId,
    worktrees,
    activeWorktreePath: ws.activeWorktreePath || (worktrees[0]?.path ?? null),
    sessions: referencedSessions,
    layout: activeLayout,
    worktreeLayouts,
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: restoredActivity,
  };
}
