import { useEffect, useSyncExternalStore } from "react";

import { deserializeWorkspaceState, serializeWorkspaceState } from "../lib/sessionPersistence";
import { resetAgentAutoResumeGuard } from "../lib/agentAutoResume";
import { loadGeneralSettings } from "../lib/generalSettings";
import { isPairedWorkspaceId, isRemoteWorkspaceId } from "../lib/remoteProject";
import {
  createStandbyBackendSessionId,
  registerSessionSnapshot,
  setSessionSleeping,
} from "../lib/sessionLifecycle";
import { isTauriRuntime, listTerminalSessions, loadSession } from "../lib/tauri";
import { defaultTauriTransport } from "../lib/terminalTransport/tauriTransport";
import type { PersistedWorkspaceSession } from "../lib/types";
import { switchDebug } from "../lib/switchDebug";
import { getHmrWorkspaceState } from "./hmrWorkspaceState";
import { getWorkspaceSnapshot, setWorkspaceSnapshot } from "./workspaceSnapshotCache";
import type { WorkspaceState } from "./workspaceStore";

export type WorkspaceRestoreStatus = "idle" | "loading" | "restored" | "failed";

const restoreStatusByWorkspace = new Map<string, WorkspaceRestoreStatus>();
const preloadedRestoreStateByWorkspace = new Map<string, WorkspaceState | null>();
const restoreStatusListeners = new Set<() => void>();
function subscribeRestoreStatus(listener: () => void) {
  restoreStatusListeners.add(listener);
  return () => { restoreStatusListeners.delete(listener); };
}

export function getWorkspaceRestoreStatus(workspaceId: string): WorkspaceRestoreStatus {
  return restoreStatusByWorkspace.get(workspaceId) ?? "idle";
}

export function setWorkspaceRestoreStatus(workspaceId: string, status: WorkspaceRestoreStatus): void {
  if (getWorkspaceRestoreStatus(workspaceId) === status) return;
  restoreStatusByWorkspace.set(workspaceId, status);
  for (const listener of restoreStatusListeners) listener();
}

/**
 * Switching back to a project must recover its tabs. When the in-memory snapshot
 * for that workspace is gone (fresh mount, cleared cache), a previously
 * "restored" workspace has to read from disk again instead of short-circuiting.
 */
export function reopenWorkspaceRestore(workspaceId: string): void {
  if (restoreStatusByWorkspace.get(workspaceId) === "restored") {
    setWorkspaceRestoreStatus(workspaceId, "idle");
  }
}

export function resetWorkspaceRestore(workspaceId?: string): void {
  if (workspaceId) {
    restoreStatusByWorkspace.delete(workspaceId);
    preloadedRestoreStateByWorkspace.delete(workspaceId);
  } else {
    restoreStatusByWorkspace.clear();
    preloadedRestoreStateByWorkspace.clear();
  }
  resetAgentAutoResumeGuard(workspaceId);
}

export type UseWorkspaceRestoreOptions = {
  workspaceId: string;
  recoveredFromHmr: boolean;
  restoreWorkspace: (state: WorkspaceState) => void;
  loadSessionFn?: () => Promise<unknown>;
  listLiveBackendSessionIdsFn?: () => Promise<
    | Iterable<string | { sessionId: string; daemonEpoch?: string | null; worktreePath?: string | null; running?: boolean }>
    | {
        complete?: boolean;
        epoch?: string | null;
        daemonEpoch?: string | null;
        sessionIds?: Iterable<string>;
        sessions?: Iterable<string | { sessionId: string; daemonEpoch?: string | null; worktreePath?: string | null; running?: boolean }>;
      }
    | null
  >;
  enabled?: boolean;
};

export async function defaultListLiveBackendSessionIds(): Promise<Array<{ sessionId: string; daemonEpoch?: string | null; worktreePath?: string | null; running?: boolean }>> {
  if (isTauriRuntime()) {
    const liveSummaries = await listTerminalSessions();
    return liveSummaries.map((candidate) => ({
      sessionId: candidate.sessionId,
      daemonEpoch: candidate.daemonEpoch ?? null,
      worktreePath: candidate.worktreePath ?? null,
      running: candidate.running ?? true,
    }));
  }
  const liveSessions = await defaultTauriTransport.listSessions();
  return liveSessions.map((candidate) => ({
    sessionId: candidate.sessionId,
    daemonEpoch: candidate.daemonEpoch ?? null,
    worktreePath: candidate.worktreePath ?? null,
    running: (candidate as any).running ?? true,
  }));
}

function activeRestoreSessionIds(state: WorkspaceState): Set<string> {
  const sessionIds = new Set<string>();
  const focusedGroupId = state.layout.focusedGroupId;
  const activeTabId = focusedGroupId
    ? state.layout.tabGroups?.[focusedGroupId]?.activeTabId ?? state.layout.activeTabId
    : state.layout.activeTabId;
  if (!activeTabId) return sessionIds;
  const tab = state.layout.tabs.find((candidate) => candidate.id === activeTabId);
  if (!tab || tab.kind === "browser") return sessionIds;
  sessionIds.add(tab.sessionId);
  const tabLayout = state.layout.layoutsByTabId?.[activeTabId];
  for (const sessionId of Object.values(tabLayout?.sessionIdsByLeafId ?? {})) {
    if (sessionId) sessionIds.add(sessionId);
  }
  return sessionIds;
}

function prepareDiskRestoredState(workspaceId: string, state: WorkspaceState): WorkspaceState {
  const policy = loadGeneralSettings().sessionRestorePolicy;
  const activeSessionIds = activeRestoreSessionIds(state);
  const sessions = Object.fromEntries(
    Object.entries(state.sessions).map(([id, session]) => {
      const isLocal = !isRemoteWorkspaceId(session.workspaceId) && !isPairedWorkspaceId(session.workspaceId);
      const isPaired = isPairedWorkspaceId(session.workspaceId);
      const isMissing = session.backendSessionId === null;
      const shouldSleep = (isLocal || isPaired) && isMissing && (
        policy === "lazy" || (policy === "activeOnly" && !activeSessionIds.has(id))
      );
      // Every sleeping local session gets a frontend-only standby identity. This prevents
      // App's legacy eager-recovery pass from spawning either shells or fallback shells for
      // agents. Agent reconnect logic treats this identity as process-absent and can resume
      // the provider session transparently when the pane is focused.
      const nextSession = {
        ...session,
        lastOutputSequence: null,
        ...(shouldSleep
          ? {
              backendSessionId: createStandbyBackendSessionId(id),
              processState: session.processState === "hibernated" ? "hibernated" as const : "standby" as const,
              lifecycle: "exited" as const,
              reconnectLifecycle: "idle" as const,
            }
          : {}),
      };
      setSessionSleeping(id, shouldSleep);
      registerSessionSnapshot(nextSession, state.activityBySessionId?.[id]?.state);
      return [id, nextSession];
    }),
  );
  return {
    ...state,
    workspaceId,
    sessions,
  };
}

export async function preloadWorkspaceSnapshots(
  workspaceIds: readonly string[],
  loadSessionFn: () => Promise<unknown> = loadSession,
  listLiveBackendSessionIdsFn: UseWorkspaceRestoreOptions["listLiveBackendSessionIdsFn"] =
    defaultListLiveBackendSessionIds,
): Promise<void> {
  switchDebug("workspace.preload.start", { workspaceIds });
  const persistedSession = (await loadSessionFn()) as PersistedWorkspaceSession | null;
  if (!persistedSession) {
    switchDebug("workspace.preload.empty", { workspaceIds });
    for (const workspaceId of workspaceIds) {
      preloadedRestoreStateByWorkspace.set(workspaceId, null);
    }
    return;
  }
  let liveBackendIds;
  try { liveBackendIds = await listLiveBackendSessionIdsFn(); }
  catch (error) {
    if (!workspaceIds.every(id => id.startsWith("ssh:")) &&
      !workspaceIds.some(id => persistedSession.workspaces[id]?.target?.kind === "pairedDaemon")) throw error;
    console.warn("Remote restore deferred daemon reconciliation:", error);
    liveBackendIds = null;
  }
  switchDebug("workspace.preload.loaded", {
    workspaceIds,
    persistedWorkspaceIds: Object.keys(persistedSession.workspaces ?? {}),
    liveBackendShape: Array.isArray(liveBackendIds)
      ? `array:${liveBackendIds.length}`
      : liveBackendIds === null
        ? "null"
        : typeof liveBackendIds,
  });

  for (const workspaceId of workspaceIds) {
    const restoredState = deserializeWorkspaceState(
      workspaceId,
      persistedSession,
      liveBackendIds,
    );
    if (!restoredState) {
      switchDebug("workspace.preload.missing", { workspaceId });
      preloadedRestoreStateByWorkspace.set(workspaceId, null);
      continue;
    }
    const preparedState = prepareDiskRestoredState(workspaceId, restoredState);
    setWorkspaceSnapshot(
      workspaceId,
      preparedState,
    );
    preloadedRestoreStateByWorkspace.set(workspaceId, preparedState);
    switchDebug("workspace.preload.snapshot", {
      workspaceId,
      activeWorktreePath: preparedState.activeWorktreePath,
      worktreeCount: preparedState.worktrees.length,
      tabCount: preparedState.layout.tabs.length,
      tabIds: preparedState.layout.tabs.map((tab) => tab.id),
      sessionCount: Object.keys(preparedState.sessions).length,
      missingBackendSessionIds: Object.values(preparedState.sessions)
        .filter((session) => session.backendSessionId === null)
        .map((session) => session.id),
    });
  }
  switchDebug("workspace.preload.complete", { workspaceIds });
}

export function useWorkspaceRestore({
  workspaceId,
  recoveredFromHmr,
  restoreWorkspace,
  loadSessionFn = loadSession,
  listLiveBackendSessionIdsFn = defaultListLiveBackendSessionIds,
  enabled = true,
}: UseWorkspaceRestoreOptions): WorkspaceRestoreStatus {
  useEffect(() => {
    if (!enabled) {
      switchDebug("workspace.restore.gated", { workspaceId });
      return;
    }

    if (preloadedRestoreStateByWorkspace.has(workspaceId)) {
      const preloadedState = preloadedRestoreStateByWorkspace.get(workspaceId) ?? null;
      preloadedRestoreStateByWorkspace.delete(workspaceId);

      if (recoveredFromHmr) {
        switchDebug("workspace.restore.preloaded.skipped-hmr", {
          workspaceId,
          action: "skipped-hmr",
          hasState: Boolean(preloadedState),
          tabCount: preloadedState?.layout.tabs.length ?? 0,
          sessionCount: preloadedState ? Object.keys(preloadedState.sessions).length : 0,
        });
        setWorkspaceRestoreStatus(workspaceId, "restored");
        return;
      }

      switchDebug("workspace.restore.preloaded", {
        workspaceId,
        action: "applied",
        hasState: Boolean(preloadedState),
        tabCount: preloadedState?.layout.tabs.length ?? 0,
        sessionCount: preloadedState ? Object.keys(preloadedState.sessions).length : 0,
      });
      if (preloadedState) restoreWorkspace(preloadedState);
      setWorkspaceRestoreStatus(workspaceId, "restored");
      return;
    }

    if (!recoveredFromHmr && getWorkspaceSnapshot(workspaceId) === null) {
      reopenWorkspaceRestore(workspaceId);
    }
    const currentStatus = getWorkspaceRestoreStatus(workspaceId);
    if (currentStatus === "restored") {
      switchDebug("workspace.restore.skipped", {
        workspaceId,
        status: currentStatus,
      });
      return;
    }

    let cancelled = false;
    setWorkspaceRestoreStatus(workspaceId, "loading");
    switchDebug("workspace.restore.start", {
      workspaceId,
      recoveredFromHmr,
    });

    async function runRestore() {
      try {
        let session: any = null;
        if (recoveredFromHmr) {
          const hmrState = getHmrWorkspaceState(workspaceId);
          if (!hmrState) {
            setWorkspaceRestoreStatus(workspaceId, "restored");
            switchDebug("workspace.restore.complete", {
              workspaceId,
              source: "hmr-empty",
            });
            return;
          }
          session = serializeWorkspaceState(
            workspaceId,
            hmrState.activeWorktreePath ?? "",
            hmrState,
          );
        } else {
          session = (await loadSessionFn()) as any;
        }
        if (cancelled) return;

        if (!session) {
          setWorkspaceRestoreStatus(workspaceId, "restored");
          switchDebug("workspace.restore.complete", {
            workspaceId,
            source: "disk-empty",
          });
          return;
        }

        let liveBackendIds;
        try { liveBackendIds = await listLiveBackendSessionIdsFn(); }
        catch (error) {
          if (!workspaceId.startsWith("ssh:") && session.workspaces?.[workspaceId]?.target?.kind !== "pairedDaemon") throw error;
          console.warn("Remote restore deferred daemon reconciliation:", error);
          liveBackendIds = null;
        }
        if (cancelled) return;
        switchDebug("workspace.restore.loaded", {
          workspaceId,
          source: recoveredFromHmr ? "hmr" : "disk",
          liveBackendShape: Array.isArray(liveBackendIds)
            ? `array:${liveBackendIds.length}`
            : liveBackendIds === null
              ? "null"
              : typeof liveBackendIds,
        });

        const restoredState = deserializeWorkspaceState(workspaceId, session, liveBackendIds);
        if (cancelled) return;

        const hasRestoredTabs =
          restoredState &&
          (restoredState.layout.tabs.length > 0 ||
            Object.values(restoredState.worktreeLayouts ?? {}).some((l) => l.tabs.length > 0));
        switchDebug("workspace.restore.resolved", {
          workspaceId,
          hasRestoredTabs: Boolean(hasRestoredTabs),
          tabCount: restoredState?.layout.tabs.length ?? 0,
          parkedTabCount: restoredState
            ? Object.values(restoredState.worktreeLayouts ?? {}).reduce(
                (total, layout) => total + layout.tabs.length,
                0,
              )
            : 0,
          sessionCount: restoredState ? Object.keys(restoredState.sessions).length : 0,
        });

        if (hasRestoredTabs) {
          const stateToRestore = recoveredFromHmr
            ? restoredState
            : prepareDiskRestoredState(workspaceId, restoredState);
          restoreWorkspace(stateToRestore);
          setWorkspaceRestoreStatus(workspaceId, "restored");
          switchDebug("workspace.restore.complete", {
            workspaceId,
            source: recoveredFromHmr ? "hmr" : "disk",
          });
          return;
        }

        setWorkspaceRestoreStatus(workspaceId, "restored");
        switchDebug("workspace.restore.complete", {
          workspaceId,
          source: "no-tabs",
        });
      } catch (error) {
        if (!cancelled) {
          setWorkspaceRestoreStatus(workspaceId, "failed");
          switchDebug("workspace.restore.error", {
            workspaceId,
            error: String(error),
          });
          console.warn("Session restore on boot skipped:", error);
        }
      }
    }

    void runRestore();

    return () => {
      cancelled = true;
      switchDebug("workspace.restore.cancel", {
        workspaceId,
        status: getWorkspaceRestoreStatus(workspaceId),
      });
      if (getWorkspaceRestoreStatus(workspaceId) === "loading") {
        setWorkspaceRestoreStatus(workspaceId, "idle");
      }
    };
  }, [enabled, workspaceId, recoveredFromHmr, restoreWorkspace, loadSessionFn, listLiveBackendSessionIdsFn]);

  return useSyncExternalStore(subscribeRestoreStatus, () => getWorkspaceRestoreStatus(workspaceId));
}
