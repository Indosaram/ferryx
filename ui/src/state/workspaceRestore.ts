import { useEffect } from "react";

import { deserializeWorkspaceState, serializeWorkspaceState } from "../lib/sessionPersistence";
import { resetAgentAutoResumeGuard } from "../lib/agentAutoResume";
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

export function getWorkspaceRestoreStatus(workspaceId: string): WorkspaceRestoreStatus {
  return restoreStatusByWorkspace.get(workspaceId) ?? "idle";
}

export function setWorkspaceRestoreStatus(workspaceId: string, status: WorkspaceRestoreStatus): void {
  restoreStatusByWorkspace.set(workspaceId, status);
}

/**
 * Switching back to a project must recover its tabs. When the in-memory snapshot
 * for that workspace is gone (fresh mount, cleared cache), a previously
 * "restored" workspace has to read from disk again instead of short-circuiting.
 */
export function reopenWorkspaceRestore(workspaceId: string): void {
  if (restoreStatusByWorkspace.get(workspaceId) === "restored") {
    restoreStatusByWorkspace.delete(workspaceId);
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
    | Iterable<string | { sessionId: string; daemonEpoch?: string | null }>
    | {
        epoch?: string | null;
        daemonEpoch?: string | null;
        sessionIds?: Iterable<string>;
        sessions?: Iterable<string | { sessionId: string; daemonEpoch?: string | null }>;
      }
    | null
  >;
  enabled?: boolean;
};

export async function defaultListLiveBackendSessionIds(): Promise<Array<{ sessionId: string; daemonEpoch?: string | null; worktreePath?: string | null }>> {
  if (isTauriRuntime()) {
    const liveSummaries = await listTerminalSessions();
    return liveSummaries.map((candidate) => ({
      sessionId: candidate.sessionId,
      daemonEpoch: candidate.daemonEpoch ?? null,
      worktreePath: candidate.worktreePath ?? null,
    }));
  }
  const liveSessions = await defaultTauriTransport.listSessions();
  return liveSessions.map((candidate) => ({
    sessionId: candidate.sessionId,
    daemonEpoch: candidate.daemonEpoch ?? null,
    worktreePath: candidate.worktreePath ?? null,
  }));
}

function prepareDiskRestoredState(workspaceId: string, state: WorkspaceState): WorkspaceState {
  return {
    ...state,
    workspaceId,
    sessions: Object.fromEntries(
      Object.entries(state.sessions).map(([id, session]) => [
        id,
        { ...session, lastOutputSequence: null },
      ]),
    ),
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
  const liveBackendIds = await listLiveBackendSessionIdsFn();
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

        const liveBackendIds = await listLiveBackendSessionIdsFn();
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

  return getWorkspaceRestoreStatus(workspaceId);
}
