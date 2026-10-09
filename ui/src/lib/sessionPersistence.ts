import { hasValidProjectTarget } from "./projectIdentity";
import { localSplitIntent } from "./localSplitLifecycle";
import { createLayoutState, normalizeLayout } from "../state/layout";
import { collectLeafIds, createLeafNode, removeLeaf, type PaneNode } from "../state/paneTree";
import type { WorkspaceState } from "../state/workspaceStore";
import type { TerminalActivity } from "./activity";
import {
  isNavigableBrowserUrl,
  loadBrowserSettings,
  PRIVATE_BROWSER_PROFILE,
  resolveSupportedBrowserProfileId,
  supportedBrowserProfiles,
} from "./browserSettings";
import { getBrowserScroll, type BrowserScrollPosition } from "./browserHistory";
import { normalizeSessionId, providerSessionKeyForAgent } from "./agentResume";
import { isAbsoluteTerminalCwd } from "./terminalCwd";
import { getSessionProcessState, getSessionRecentScrollback, restoreSessionRecentScrollback } from "./sessionLifecycle";
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
  type SessionProcessState,
  type TerminalLifecycle,
  type TerminalSession,
  type TerminalTab,
  type WorkspaceTab,
  type Worktree,
} from "./types";

export const WORKSPACE_SESSION_VERSION = 3;

declare module "./types" {
  interface PersistedBrowserTabState {
    scrollX?: number;
    scrollY?: number;
    scrollPosition?: BrowserScrollPosition;
  }
}

export function isPrivateBrowserProfileId(profileId: string | null | undefined): boolean {
  const trimmed = profileId?.trim();
  return trimmed === PRIVATE_BROWSER_PROFILE.id || trimmed === "private" || trimmed === "remote";
}

/**
 * Identity of everything about the live sessions that has to reach disk. Saves are scheduled off
 * this key, so anything missing from it can change in memory and never be persisted. The agent
 * resume identity belongs here because an agent starts a new conversation inside the same pane
 * (`/new`) without touching its backend id or lifecycle.
 */
export function sessionPersistenceKey(sessions: Record<string, TerminalSession>): string {
  return Object.entries(sessions)
    .map(
      ([id, session]) =>
        `${id}:${session.backendSessionId ?? ""}:${session.daemonEpoch ?? ""}:${session.incarnation ?? ""}:${getSessionProcessState(session)}:${session.lifecycle}:${session.agentType ?? ""}:${session.providerSession?.key ?? ""}:${session.providerSession?.id ?? ""}:${JSON.stringify(localSplitIntent(session) ?? null)}:${JSON.stringify(session.attachTuple ?? null)}`,
    )
    .join(",");
}

export function serializeWorkspaceState(
  workspaceId: string,
  repoRoot: string,
  state: WorkspaceState,
  existingSession?: PersistedWorkspaceSession | null,
  project?: Pick<import("./types").RegisteredProject, "target" | "remoteWorkspaceId" | "gitRoot" | "gitRemote" | "gitCommonDir" | "gitBranch" | "gitHead" | "hostLabel">,
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

  function serializeLayout(layoutState: LayoutState, layoutWorktreePath?: string | null): PersistedLayout {
    const normalizedLayout = normalizeLayout(layoutState);
    const persistedTabs: PersistedTab[] = [];
    for (const tab of normalizedLayout.tabs) {
      if (tab.kind === "browser") {
        if (!restoreBrowserTabs || isPrivateBrowserProfileId(tab.profileId)) continue;
        const worktreePath = tab.worktreePath || layoutWorktreePath || undefined;
        const rawScrollPos = (tab as any).scrollPosition ?? (
          typeof (tab as any).scrollX === "number" || typeof (tab as any).scrollY === "number"
            ? { x: (tab as any).scrollX ?? 0, y: (tab as any).scrollY ?? 0 }
            : getBrowserScroll(tab.browserId, tab.url)
        );
        const scrollPosition: BrowserScrollPosition | undefined = rawScrollPos
          ? {
              x: Math.max(0, Math.round(Number.isFinite(rawScrollPos.x) ? rawScrollPos.x : 0)),
              y: Math.max(0, Math.round(Number.isFinite(rawScrollPos.y) ? rawScrollPos.y : 0)),
            }
          : undefined;
        persistedTabs.push({
          id: tab.id,
          kind: "browser",
          label: tab.label,
          worktreePath,
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
            worktreePath,
            worktreeLabel: tab.worktreeLabel,
            scrollPosition,
            scrollX: scrollPosition?.x,
            scrollY: scrollPosition?.y,
          },
        });
        continue;
      }

      if (tab.kind === "file") {
        persistedTabs.push({
          id: tab.id,
          kind: "file",
          label: tab.label,
          pinned: Boolean(tab.pinned),
          file: {
            path: tab.path,
            backendSessionId: tab.backendSessionId,
            line: tab.line,
            col: tab.col,
            workspaceId: tab.workspaceId,
            previewId: tab.previewId,
          },
        });
        continue;
      }

      const tabLayout = normalizedLayout.layoutsByTabId[tab.id];
      let effectiveRoot: PaneNode | null = tabLayout?.root ?? createLeafNode(`leaf-persisted:${tab.id}`);
      let effectiveContents = tabLayout?.contentsByLeafId;

      if (tabLayout && effectiveContents) {
        for (const [leafId, content] of Object.entries(effectiveContents)) {
          if (content.kind === "browser") {
            const rawBrowser = content.browser ?? content;
            if (!restoreBrowserTabs || isPrivateBrowserProfileId(rawBrowser.profileId)) {
              const nextRoot = removeLeaf(effectiveRoot, leafId);
              if (!nextRoot) {
                effectiveRoot = null;
                break;
              }
              effectiveRoot = nextRoot;
            }
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
            const worktreePath = rawBrowser.worktreePath || layoutWorktreePath || undefined;
            const rawScrollPos = (rawBrowser as any).scrollPosition ?? (
              typeof (rawBrowser as any).scrollX === "number" || typeof (rawBrowser as any).scrollY === "number"
                ? { x: (rawBrowser as any).scrollX ?? 0, y: (rawBrowser as any).scrollY ?? 0 }
                : getBrowserScroll(rawBrowser.browserId ?? "", rawBrowser.url ?? "")
            );
            const scrollPosition: BrowserScrollPosition | undefined = rawScrollPos
              ? {
                  x: Math.max(0, Math.round(Number.isFinite(rawScrollPos.x) ? rawScrollPos.x : 0)),
                  y: Math.max(0, Math.round(Number.isFinite(rawScrollPos.y) ? rawScrollPos.y : 0)),
                }
              : undefined;
            contentsByLeafId[leafId] = createBrowserPaneContent({
              browserId: rawBrowser.browserId ?? "",
              url: rawBrowser.url ?? "",
              title: rawBrowser.title ?? null,
              loading: rawBrowser.loading ?? false,
              canGoBack: rawBrowser.canGoBack ?? false,
              canGoForward: rawBrowser.canGoForward ?? false,
              zoomFactor: rawBrowser.zoomFactor,
              profileId: rawBrowser.profileId,
              worktreePath,
              worktreeLabel: rawBrowser.worktreeLabel,
              ...(scrollPosition ? { scrollPosition, scrollX: scrollPosition.x, scrollY: scrollPosition.y } : {}),
            } as any);
            sessionIdsByLeafId[leafId] = "";
          } else if (content.kind === "dag") {
            const rawDag = content.dag ?? content;
            contentsByLeafId[leafId] = createDagPaneContent({
              runId: rawDag.runId ?? null,
            });
            sessionIdsByLeafId[leafId] = "";
          } else if (content.kind === "file") {
            contentsByLeafId[leafId] = content;
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
        ...(tab.customLabel ? { customTitle: tab.customLabel } : {}),
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

  const persistedLayout = serializeLayout(state.layout, state.activeWorktreePath);
  const persistedWorktreeLayouts: Record<string, PersistedLayout> = {};
  for (const [wtPath, layout] of Object.entries(state.worktreeLayouts ?? {})) {
    if (layout) {
      persistedWorktreeLayouts[wtPath] = serializeLayout(layout, wtPath);
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
    if (!sess || (!referencedSessionIds.has(id) && !localSplitIntent(sess))) continue;
    const activity = state.activityBySessionId?.[id];
    const agentType = sess.agentType ?? (activity?.isAgent && activity?.agentType ? activity.agentType : null);
    persistedTerminalSessions[id] = {
      localSessionId: sess.id,
      backendSessionId: sess.backendSessionId,
      processState: getSessionProcessState(sess),
      worktreePath: sess.worktreePath ?? sess.cwd,
      cwd: sess.cwd,
      daemonEpoch: sess.daemonEpoch != null ? String(sess.daemonEpoch) : null,
      lastOutputSequence: sess.lastOutputSequence != null ? String(sess.lastOutputSequence) : null,
      agentType,
      agentSessionId: sess.agentSessionId ?? null,
      providerSession: sess.providerSession ?? null,
      recentScrollback: getSessionRecentScrollback(id),
      createdAt,
      incarnation: sess.incarnation ?? null,
      attachTuple: sess.attachTuple,
      spawnIntent: localSplitIntent(sess),
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

  const target = project?.target ?? existingSession?.workspaces[workspaceId]?.target;
  const workspace: PersistedWorkspace = {
    workspaceId,
    repoRoot,
    target: target?.kind === "pairedDaemon" ? { kind: "pairedDaemon", hostId: target.hostId } : target,
    remoteWorkspaceId: project?.remoteWorkspaceId ?? existingSession?.workspaces[workspaceId]?.remoteWorkspaceId,
    gitRoot: project?.gitRoot === undefined ? existingSession?.workspaces[workspaceId]?.gitRoot : project.gitRoot,
    gitRemote: project?.gitRemote === undefined ? existingSession?.workspaces[workspaceId]?.gitRemote : project.gitRemote,
    gitCommonDir: project?.gitCommonDir === undefined ? existingSession?.workspaces[workspaceId]?.gitCommonDir : project.gitCommonDir,
    gitBranch: project?.gitBranch === undefined ? existingSession?.workspaces[workspaceId]?.gitBranch : project.gitBranch,
    gitHead: project?.gitHead === undefined ? existingSession?.workspaces[workspaceId]?.gitHead : project.gitHead,
    hostLabel: project?.hostLabel === undefined ? existingSession?.workspaces[workspaceId]?.hostLabel : project.hostLabel,
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

/**
 * Persisted-before-retirement sessions carry agentType "gemini"; restore maps it
 * to "antigravity" so old panes get the Antigravity resume affordance
 * (agy --conversation <id>) instead of a dead "cannot be reconnected" card.
 */
export function migrateLegacyAgentType(agentType: string | null | undefined): string | null | undefined {
  if (agentType == null) return agentType;
  if (agentType.trim().toLowerCase() === "gemini") return "antigravity";
  return agentType;
}

export function deserializeWorkspaceState(
  workspaceId: string,
  persistedSession: PersistedWorkspaceSession,
  liveBackendSessionIds?:
    | Iterable<string | { sessionId: string; incarnation?: string | null; daemonEpoch?: string | null; worktreePath?: string | null; running?: boolean }>
    | {
        authoritative?: boolean;
        complete?: boolean;
        epoch?: string | null;
        daemonEpoch?: string | null;
        sessionIds?: Iterable<string>;
        sessions?: Iterable<string | { sessionId: string; incarnation?: string | null; daemonEpoch?: string | null; worktreePath?: string | null; running?: boolean }>;
      }
    | Map<string, { incarnation?: string | null; daemonEpoch?: string | null; running?: boolean } | boolean>
    | null,
): WorkspaceState | null {
  const ws = persistedSession.workspaces?.[workspaceId];
  if (!ws) return null;
  if ((workspaceId.startsWith("daemon:") || ws.target?.kind === "pairedDaemon") &&
    !hasValidProjectTarget({ ...ws, workspaceId })) return null;
  const isV2 = (persistedSession.version ?? 1) >= 2;

  const browserSettings = loadBrowserSettings();
  const restoreBrowser = browserSettings.restoreTabsOnLaunch;
  const supportedProfiles = supportedBrowserProfiles(browserSettings);
  const knownProfiles = new Set(supportedProfiles.map((profile) => profile.id));
  const fallbackProfileId = resolveSupportedBrowserProfileId(browserSettings.defaultProfileId, browserSettings);

  let globalLiveEpoch: string | null = null;
  const liveSessionMap = new Map<string, { daemonEpoch: string | null; running: boolean; incarnation?: string | null }>();
  const hasLiveSessionQuery = liveBackendSessionIds !== null && liveBackendSessionIds !== undefined;
  let isAuthoritative = hasLiveSessionQuery;
  if (typeof liveBackendSessionIds === "object" && liveBackendSessionIds !== null) {
    if ("authoritative" in liveBackendSessionIds && (liveBackendSessionIds as any).authoritative === false) {
      isAuthoritative = false;
    } else if ("complete" in liveBackendSessionIds && (liveBackendSessionIds as any).complete === false) {
      isAuthoritative = false;
    }
  }

  if (hasLiveSessionQuery && liveBackendSessionIds) {
    if (
      liveBackendSessionIds instanceof Map ||
      (typeof liveBackendSessionIds === "object" &&
        "has" in liveBackendSessionIds &&
        "get" in liveBackendSessionIds &&
        typeof (liveBackendSessionIds as any).get === "function" &&
        typeof (liveBackendSessionIds as any).entries === "function")
    ) {
      for (const [key, value] of (liveBackendSessionIds as Map<any, any>).entries()) {
        const itemEpoch = typeof value === "object" && value !== null && value.daemonEpoch != null ? String(value.daemonEpoch) : globalLiveEpoch;
        const isRunning = typeof value === "object" && value !== null && "running" in value ? (value as any).running !== false : value !== false;
        liveSessionMap.set(String(key), { daemonEpoch: itemEpoch, running: isRunning, incarnation: typeof value === "object" ? value?.incarnation : null });
      }
    } else if (
      typeof liveBackendSessionIds === "object" &&
      !("length" in liveBackendSessionIds) &&
      !liveBackendSessionIds[Symbol.iterator as keyof typeof liveBackendSessionIds]
    ) {
      const container = liveBackendSessionIds as {
        authoritative?: boolean;
        complete?: boolean;
        epoch?: string | null;
        daemonEpoch?: string | null;
        sessionIds?: Iterable<string>;
        sessions?: Iterable<string | { sessionId: string; incarnation?: string | null; daemonEpoch?: string | null; worktreePath?: string | null; running?: boolean }>;
      };
      const rawEpoch = container.epoch ?? container.daemonEpoch;
      if (rawEpoch != null) {
        globalLiveEpoch = String(rawEpoch);
      }
      const sessionList = container.sessions ?? container.sessionIds ?? [];
      for (const item of sessionList) {
        if (typeof item === "string") {
          liveSessionMap.set(item, { daemonEpoch: globalLiveEpoch, running: true });
        } else if (item && typeof item === "object" && "sessionId" in item) {
          const itemEpoch = item.daemonEpoch != null ? String(item.daemonEpoch) : globalLiveEpoch;
          const isRunning = (item as any).running !== false;
          liveSessionMap.set(item.sessionId, { daemonEpoch: itemEpoch, running: isRunning, incarnation: item.incarnation });
        }
      }
    } else {
      for (const item of liveBackendSessionIds as Iterable<any>) {
        if (typeof item === "string") {
          liveSessionMap.set(item, { daemonEpoch: null, running: true });
        } else if (item && typeof item === "object" && "sessionId" in item) {
          const itemEpoch = item.daemonEpoch != null ? String(item.daemonEpoch) : null;
          if (itemEpoch !== null && globalLiveEpoch === null) {
            globalLiveEpoch = itemEpoch;
          }
          const isRunning = item.running !== false;
          liveSessionMap.set(item.sessionId, { daemonEpoch: itemEpoch, running: isRunning, incarnation: item.incarnation });
        }
      }
    }
  }

  const worktrees: Worktree[] = (ws.worktrees || []).map((wt) => ({
    // HMR snapshots predate project metadata but retain the reserved backend ID.
    ...(ws.target?.kind === "pairedDaemon" || ws.target?.kind === "ssh" || workspaceId.startsWith("ssh:") ? { workspaceId } : {}),
    path: wt.path,
    branch: wt.branch ? (wt.branch.startsWith("refs/heads/") ? wt.branch : `refs/heads/${wt.branch}`) : null,
    head: wt.head,
    bare: false,
    detached: false,
    locked: wt.isLocked ? "locked" : null,
    prunable: null,
  }));
  const knownWorktreePaths = new Set(worktrees.map((wt) => wt.path));

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
    let processState: SessionProcessState = sess.processState === "suspended" ? "suspended" : sess.processState === "hibernated" ? "hibernated" : "standby";

    const isSshSession = ws.target?.kind === "ssh" || workspaceId.startsWith("ssh:");
    const isPairedSession = ws.target?.kind === "pairedDaemon";
    let isReconnecting = false;
    if (isPairedSession) {
      // Local inventory is not authority for remote liveness or remote epochs.
      // Retain the proxy binding for exact-target reattach, never create a shell.
      backendSessionId = persistedBackendSessionId;
      daemonEpoch = persistedEpoch;
      lastOutputSequence = null;
      lifecycle = persistedBackendSessionId ? "working" : "exited";
      processState = persistedBackendSessionId ? "running" : processState;
    } else if (isSshSession && persistedBackendSessionId) {
      // SSH backend IDs identify persisted remote targets across daemon epochs.
      // List absence/running=false cannot establish remote process death; status can.
      backendSessionId = persistedBackendSessionId;
      daemonEpoch = liveSessionMap.get(persistedBackendSessionId)?.daemonEpoch ?? persistedEpoch;
      lastOutputSequence = daemonEpoch === persistedEpoch ? persistedSequence : null;
      lifecycle = "working";
      processState = "running";
    } else if (!hasLiveSessionQuery || !isAuthoritative) {
      // When we have not heard an authoritative daemon answer (e.g. during launch / handover
      // reconciliation), a persisted session is assumed alive and treated as reconnecting.
      // Keep its persisted backendSessionId so the user can reattach or recover.
      const isLive = Boolean(persistedBackendSessionId);
      backendSessionId = isLive ? persistedBackendSessionId : null;
      daemonEpoch = isLive ? persistedEpoch : null;
      lastOutputSequence = isLive ? (daemonEpoch === persistedEpoch ? persistedSequence : null) : null;
      lifecycle = isLive ? "working" : "exited";
      processState = isLive ? "running" : processState;
      if (isLive && (!hasLiveSessionQuery || !liveSessionMap.has(persistedBackendSessionId!))) {
        isReconnecting = true;
      }
    } else {
      if (!persistedBackendSessionId || !liveSessionMap.has(persistedBackendSessionId)) {
        backendSessionId = null;
        daemonEpoch = null;
        lastOutputSequence = null;
        lifecycle = "exited";
      } else {
        const liveInfo = liveSessionMap.get(persistedBackendSessionId);
        const effectiveLiveEpoch = liveInfo?.daemonEpoch ?? globalLiveEpoch;
        const isProcessRunning = liveInfo ? liveInfo.running !== false : true;

        // A hit in liveSessionMap came from listSessions on the daemon running right now, and
        // backend ids do not survive a daemon restart. So a live hit already proves the PTY is
        // ours; only a RECORDED epoch that disagrees can disprove it.
        //
        // Treating a missing persisted epoch as a mismatch orphaned every restored session,
        // because no reducer writes daemonEpoch onto a session -- it is null in every save file.
        let epochMatches = true;
        if (effectiveLiveEpoch !== null && persistedEpoch !== null) {
          epochMatches = effectiveLiveEpoch === persistedEpoch || Boolean(sess.incarnation && liveInfo?.incarnation === sess.incarnation);
        } else if (liveInfo === undefined && effectiveLiveEpoch !== null && persistedEpoch === null) {
          epochMatches = false;
        }

        if (epochMatches && isProcessRunning) {
          backendSessionId = persistedBackendSessionId;
          daemonEpoch = effectiveLiveEpoch ?? persistedEpoch;
          lastOutputSequence = daemonEpoch === persistedEpoch ? persistedSequence : null;
          lifecycle = "working";
          processState = "running";
        } else {
          backendSessionId = null;
          daemonEpoch = null;
          lastOutputSequence = null;
          lifecycle = "exited";
        }
      }
    }

    if (!isSshSession && !isPairedSession && persistedBackendSessionId && lifecycle === "exited") {
      const live = liveSessionMap.get(persistedBackendSessionId);
      const epoch = live?.daemonEpoch ?? globalLiveEpoch;
      // Absence in a new epoch cannot establish death in a draining predecessor.
      if (live?.running !== false && persistedEpoch !== null && epoch !== null && epoch !== persistedEpoch &&
          (!live || !sess.incarnation || !live.incarnation)) {
        backendSessionId = persistedBackendSessionId;
        daemonEpoch = persistedEpoch;
        lastOutputSequence = persistedSequence;
        lifecycle = "working";
        processState = "running";
        isReconnecting = true;
      }
    }

    const worktreePath = sess.worktreePath || sess.cwd;
    const matchingWorktree = worktrees.find((wt) => wt.path === worktreePath);
    const activity = ws.activityBySessionId?.[localSessionId];
    const fallbackAgentType = activity?.isAgent && activity?.agentType ? activity.agentType : null;
    const agentType = migrateLegacyAgentType(sess.agentType) ?? migrateLegacyAgentType(fallbackAgentType);
    const agentSessionId = sess.agentSessionId ?? null;
    const legacyProviderKey = agentType ? providerSessionKeyForAgent(agentType) : null;
    const normalizedLegacyId = agentSessionId ? normalizeSessionId(agentSessionId) : null;
    const providerSession = sess.providerSession ?? (
      legacyProviderKey && normalizedLegacyId
        ? { key: legacyProviderKey, id: normalizedLegacyId }
        : null
    );
    restoreSessionRecentScrollback(localSessionId, sess.recentScrollback);
    // A cwd persisted before the value was validated can be probe output rather than a path
    // (`cwd|rtd info error: …`); such a value must never become a pane's cwd again.
    const restoredCwd = isAbsoluteTerminalCwd(sess.cwd) ? sess.cwd : null;
    sessions[localSessionId] = {
      id: localSessionId,
      cwd: restoredCwd ?? worktreePath,
      worktreePath,
      workspaceId,
      worktree: matchingWorktree ? worktreeIdentity(matchingWorktree) : null,
      backendSessionId,
      processState,
      lifecycle,
      daemonEpoch,
      lastOutputSequence,
      attachTuple: sess.attachTuple,
      agentType,
      agentSessionId,
      providerSession,
      reconnectLifecycle: "idle",
      reconnectError: null,
      reconnectRequestId: null,
      ...(isSshSession || isPairedSession
        ? { remoteConnectionState: persistedBackendSessionId ? ("reconnecting" as const) : ("legacyLost" as const) }
        : isReconnecting
          ? { remoteConnectionState: "reconnecting" as const }
          : {}),
    };
    const persistedIncarnation = sess.incarnation ?? null;
    const savedIntent = sess.spawnIntent;
    if (savedIntent && (!savedIntent.ready || savedIntent.cancelRequested)) {
      Object.assign(sessions[localSessionId], {
        spawnIntent: savedIntent,
        reconnectRequestId: savedIntent.requestId,
        backendSessionId: persistedBackendSessionId,
        daemonEpoch: persistedEpoch,
        incarnation: persistedIncarnation,
        lifecycle: "starting",
        reconnectLifecycle: "failed",
        reconnectError: {
          code: "SPAWN_ATTEMPT_TIMEOUT",
          message: "Shell startup requires reconciliation. Retry the original request.",
          details: { delivery: "ambiguous" },
        },
      });
    } else if (persistedIncarnation) {
      sessions[localSessionId].incarnation = persistedIncarnation;
    }
    // Completed spawns restore as ordinary durable sessions. Only unfinished or
    // cancelled requests above retain their reconciliation coordinator.
  }

  function deserializeLayout(
    persistedLayout: PersistedLayout | undefined,
    targetWorktreePath?: string | null,
  ): LayoutState {
    if (!persistedLayout) return createLayoutState();

    const tabs: WorkspaceTab[] = [];
    for (const persistedTab of persistedLayout.tabs || []) {
      if (persistedTab.kind === "browser" || persistedTab.browser) {
        if (!restoreBrowser) continue;
        const browser = persistedTab.browser;
        const rawProfileId = browser?.profileId;
        // Private-profile tabs must be excluded from restore (privacy policy)
        if (isPrivateBrowserProfileId(rawProfileId)) continue;

        const url = browser?.url ?? "about:blank";
        // Stale state check: skip invalid URLs without breaking the rest of restore
        if (!isNavigableBrowserUrl(url)) continue;

        const tabExplicitWorktree = browser?.worktreePath ?? persistedTab.worktreePath ?? undefined;
        // Stale state check: skip tab if its worktree no longer exists
        if (tabExplicitWorktree && knownWorktreePaths.size > 0 && !knownWorktreePaths.has(tabExplicitWorktree)) {
          continue;
        }
        // Worktree partition: tab belongs to its worktree, not mixed into another
        if (tabExplicitWorktree && tabExplicitWorktree !== (targetWorktreePath || undefined)) {
          continue;
        }
        const tabWorktreePath = tabExplicitWorktree ?? (targetWorktreePath || undefined);

        const profileId = rawProfileId && knownProfiles.has(rawProfileId) ? rawProfileId : fallbackProfileId;

        // Backward-compatible scroll tolerance: missing/NaN/null positions default gracefully to { x: 0, y: 0 }
        const rawScrollX = (browser as any)?.scrollX ?? (browser as any)?.scrollPosition?.x;
        const rawScrollY = (browser as any)?.scrollY ?? (browser as any)?.scrollPosition?.y;
        const historyScroll = rawScrollX === undefined && rawScrollY === undefined
          ? getBrowserScroll(browser?.browserId ?? persistedTab.sessionId ?? "", url)
          : null;
        const scrollPosition: BrowserScrollPosition = historyScroll ?? {
          x: typeof rawScrollX === "number" && Number.isFinite(rawScrollX) ? Math.max(0, Math.round(rawScrollX)) : 0,
          y: typeof rawScrollY === "number" && Number.isFinite(rawScrollY) ? Math.max(0, Math.round(rawScrollY)) : 0,
        };

        const tab: BrowserTab = {
          id: persistedTab.id,
          kind: "browser",
          label: persistedTab.label,
          browserId: browser?.browserId ?? persistedTab.sessionId ?? `restored-browser:${persistedTab.id}`,
          url,
          title: browser?.title ?? persistedTab.label,
          canGoBack: browser?.canGoBack ?? false,
          canGoForward: browser?.canGoForward ?? false,
          zoomFactor: browser?.zoomFactor,
          loading: browser?.loading ?? false,
          pinned: Boolean(persistedTab.pinned),
          profileId,
          worktreePath: tabWorktreePath,
          worktreeLabel: browser?.worktreeLabel,
          ...({ scrollPosition, scrollX: scrollPosition.x, scrollY: scrollPosition.y } as any),
        };
        tabs.push(tab);
        continue;
      }

      if (persistedTab.kind === "file" || persistedTab.file) {
        const file = persistedTab.file;
        if (!file?.path) continue;
        tabs.push({
          id: persistedTab.id,
          kind: "file",
          label: persistedTab.label,
          path: file.path,
          backendSessionId: file.backendSessionId,
          line: file.line,
          col: file.col,
          workspaceId: file.workspaceId,
          previewId: file.previewId || persistedTab.id,
          pinned: Boolean(persistedTab.pinned),
        });
        continue;
      }

      const primarySessionId = persistedTab.terminal?.primarySessionId ?? persistedTab.sessionId ?? "";
      const customLabel = typeof persistedTab.customTitle === "string" ? persistedTab.customTitle.trim().slice(0, 200) : "";
      const tab: TerminalTab = {
        id: persistedTab.id,
        kind: "terminal",
        sessionId: primarySessionId,
        label: persistedTab.label,
        pinned: Boolean(persistedTab.pinned),
        ...(customLabel ? { customLabel } : {}),
      };
      tabs.push(tab);
    }

    const layoutsByTabId: LayoutState["layoutsByTabId"] = {};
    for (const persistedTab of persistedLayout.tabs || []) {
      if (persistedTab.kind === "browser" || persistedTab.browser) {
        if (!restoreBrowser) continue;
        const browser = persistedTab.browser;
        const rawProfileId = browser?.profileId;
        if (isPrivateBrowserProfileId(rawProfileId)) continue;
        const url = browser?.url ?? "about:blank";
        if (!isNavigableBrowserUrl(url)) continue;

        const tabExplicitWorktree = browser?.worktreePath ?? persistedTab.worktreePath ?? undefined;
        if (tabExplicitWorktree && knownWorktreePaths.size > 0 && !knownWorktreePaths.has(tabExplicitWorktree)) continue;
        if (tabExplicitWorktree && tabExplicitWorktree !== (targetWorktreePath || undefined)) continue;
        const tabWorktreePath = tabExplicitWorktree ?? (targetWorktreePath || undefined);

        const profileId = rawProfileId && knownProfiles.has(rawProfileId) ? rawProfileId : fallbackProfileId;
        const leafId = `leaf-browser:${persistedTab.id}`;
        const rawScrollX = (browser as any)?.scrollX ?? (browser as any)?.scrollPosition?.x;
        const rawScrollY = (browser as any)?.scrollY ?? (browser as any)?.scrollPosition?.y;
        const historyScroll = rawScrollX === undefined && rawScrollY === undefined
          ? getBrowserScroll(browser?.browserId ?? persistedTab.sessionId ?? "", url)
          : null;
        const scrollPosition: BrowserScrollPosition = historyScroll ?? {
          x: typeof rawScrollX === "number" && Number.isFinite(rawScrollX) ? Math.max(0, Math.round(rawScrollX)) : 0,
          y: typeof rawScrollY === "number" && Number.isFinite(rawScrollY) ? Math.max(0, Math.round(rawScrollY)) : 0,
        };

        const browserState: BrowserPaneState = {
          browserId: browser?.browserId ?? persistedTab.sessionId ?? `restored-browser:${persistedTab.id}`,
          url,
          title: browser?.title ?? persistedTab.label,
          canGoBack: browser?.canGoBack ?? false,
          canGoForward: browser?.canGoForward ?? false,
          zoomFactor: browser?.zoomFactor,
          loading: browser?.loading ?? false,
          profileId,
          worktreePath: tabWorktreePath,
          worktreeLabel: browser?.worktreeLabel,
          ...({ scrollPosition, scrollX: scrollPosition.x, scrollY: scrollPosition.y } as any),
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

      if (persistedTab.kind === "file" || persistedTab.file) {
        const file = persistedTab.file;
        if (!file?.path) continue;
        const leafId = `leaf-file:${persistedTab.id}`;
        layoutsByTabId[persistedTab.id] = {
          root: createLeafNode(leafId),
          activeLeafId: leafId,
          expandedLeafId: null,
          sessionIdsByLeafId: { [leafId]: "" },
          contentsByLeafId: {
            [leafId]: {
              kind: "file",
              path: file.path,
              backendSessionId: file.backendSessionId,
              line: file.line,
              col: file.col,
              workspaceId: file.workspaceId,
              previewId: file.previewId || persistedTab.id,
            },
          },
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

      if (persistedContents) {
        for (const [leafId, content] of Object.entries(persistedContents)) {
          if (content && content.kind === "browser") {
            const rawBrowser = content.browser ?? content;
            const paneExplicitWt = rawBrowser.worktreePath;
            const paneWtPath = paneExplicitWt ?? (targetWorktreePath || undefined);
            const isStaleWt = Boolean(paneWtPath && knownWorktreePaths.size > 0 && !knownWorktreePaths.has(paneWtPath));
            const isOtherWt = Boolean(paneExplicitWt && paneExplicitWt !== (targetWorktreePath || undefined));
            if (
              !restoreBrowser ||
              isPrivateBrowserProfileId(rawBrowser.profileId) ||
              !isNavigableBrowserUrl(rawBrowser.url ?? "about:blank") ||
              isStaleWt ||
              isOtherWt
            ) {
              const nextRoot = removeLeaf(root, leafId);
              if (!nextRoot) {
                root = null;
                break;
              }
              root = nextRoot;
            }
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
            if (
              isPrivateBrowserProfileId(rawProfileId) ||
              !isNavigableBrowserUrl(rawBrowser.url ?? "about:blank")
            ) {
              continue;
            }
            const paneExplicitWt = rawBrowser.worktreePath;
            if (paneExplicitWt && paneExplicitWt !== (targetWorktreePath || undefined)) {
              continue;
            }
            const paneWtPath = paneExplicitWt ?? (targetWorktreePath || undefined);
            if (paneWtPath && knownWorktreePaths.size > 0 && !knownWorktreePaths.has(paneWtPath)) {
              continue;
            }

            const profileId = rawProfileId && knownProfiles.has(rawProfileId) ? rawProfileId : fallbackProfileId;
            const rawScrollX = (rawBrowser as any)?.scrollX ?? (rawBrowser as any)?.scrollPosition?.x;
            const rawScrollY = (rawBrowser as any)?.scrollY ?? (rawBrowser as any)?.scrollPosition?.y;
            const historyScroll = rawScrollX === undefined && rawScrollY === undefined
              ? getBrowserScroll(rawBrowser.browserId ?? "", rawBrowser.url ?? "")
              : null;
            const scrollPosition: BrowserScrollPosition = historyScroll ?? {
              x: typeof rawScrollX === "number" && Number.isFinite(rawScrollX) ? Math.max(0, Math.round(rawScrollX)) : 0,
              y: typeof rawScrollY === "number" && Number.isFinite(rawScrollY) ? Math.max(0, Math.round(rawScrollY)) : 0,
            };

            contentsByLeafId[leafId] = createBrowserPaneContent({
              browserId: rawBrowser.browserId || `restored-browser:${persistedTab.id}:${leafId}`,
              url: rawBrowser.url || "about:blank",
              title: rawBrowser.title ?? null,
              loading: Boolean(rawBrowser.loading),
              canGoBack: Boolean(rawBrowser.canGoBack),
              canGoForward: Boolean(rawBrowser.canGoForward),
              zoomFactor: typeof rawBrowser.zoomFactor === "number" ? rawBrowser.zoomFactor : undefined,
              profileId,
              worktreePath: paneWtPath,
              worktreeLabel: rawBrowser.worktreeLabel,
              ...({ scrollPosition, scrollX: scrollPosition.x, scrollY: scrollPosition.y } as any),
            });
            sessionIdsByLeafId[leafId] = "";
          } else if (rawContent.kind === "dag") {
            const rawDag = rawContent.dag ?? rawContent;
            contentsByLeafId[leafId] = createDagPaneContent({
              runId: rawDag.runId ?? null,
            });
            sessionIdsByLeafId[leafId] = "";
          } else if (rawContent.kind === "file") {
            contentsByLeafId[leafId] = rawContent;
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

  const activeLayout = deserializeLayout(ws.layout, ws.activeWorktreePath);
  const worktreeLayouts: Record<string, LayoutState> = {};
  if (ws.worktreeLayouts) {
    for (const [wtPath, persistedWtLayout] of Object.entries(ws.worktreeLayouts)) {
      if (persistedWtLayout) {
        if (knownWorktreePaths.size > 0 && !knownWorktreePaths.has(wtPath)) {
          // Stale worktree: skip layout belonging to deleted worktree
          continue;
        }
        worktreeLayouts[wtPath] = deserializeLayout(persistedWtLayout, wtPath);
      }
    }
  }

  const allLayouts = [activeLayout, ...Object.values(worktreeLayouts)];
  const referencedSessionIds = new Set<string>();
  for (const layout of allLayouts) {
    for (const tab of layout.tabs) {
      if (tab.kind === "browser" || tab.kind === "file") continue;
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
    Object.entries(sessions).filter(([sessionId, session]) => referencedSessionIds.has(sessionId) || localSplitIntent(session)),
  );

  const restoredActivity: Record<string, TerminalActivity> = {};
  if (ws.activityBySessionId) {
    for (const [sessionId, activity] of Object.entries(ws.activityBySessionId)) {
      if (activity && referencedSessionIds.has(sessionId)) {
        const session = sessions[sessionId];
        const isSessionLive = Boolean(session && session.backendSessionId !== null && session.lifecycle !== "exited");
        // If the session died or has exited across restart, any in-flight working claim is stale and settles to done.
        // If the session survived in the daemon and is still running, preserve its working state so
        // that running indicators (e.g. sidebar running count, tab spinner) reflect reality.
        const isStaleRunClaim = activity.state === "working" && !isSessionLive;
        restoredActivity[sessionId] = {
          state: isStaleRunClaim ? "done" : activity.state,
          title: activity.title || "",
          isAgent: Boolean(activity.isAgent),
          ...(activity.agentType ? { agentType: migrateLegacyAgentType(activity.agentType) ?? activity.agentType } : {}),
          ...(activity.source ? { source: activity.source } : {}),
          ...(activity.agentSource ? { agentSource: activity.agentSource } : {}),
          seen: true,
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
