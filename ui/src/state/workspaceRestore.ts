import { useEffect } from "react";

import { deserializeWorkspaceState } from "../lib/sessionPersistence";
import { isTauriRuntime, listTerminalSessions, loadSession } from "../lib/tauri";
import { defaultTauriTransport } from "../lib/terminalTransport/tauriTransport";
import type { WorkspaceState } from "./workspaceStore";

export type WorkspaceRestoreStatus = "idle" | "loading" | "restored" | "failed";

const restoreStatusByWorkspace = new Map<string, WorkspaceRestoreStatus>();

export function getWorkspaceRestoreStatus(workspaceId: string): WorkspaceRestoreStatus {
  return restoreStatusByWorkspace.get(workspaceId) ?? "idle";
}

export function setWorkspaceRestoreStatus(workspaceId: string, status: WorkspaceRestoreStatus): void {
  restoreStatusByWorkspace.set(workspaceId, status);
}

export function resetWorkspaceRestore(workspaceId?: string): void {
  if (workspaceId) {
    restoreStatusByWorkspace.delete(workspaceId);
  } else {
    restoreStatusByWorkspace.clear();
  }
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

export function useWorkspaceRestore({
  workspaceId,
  recoveredFromHmr,
  restoreWorkspace,
  loadSessionFn = loadSession,
  listLiveBackendSessionIdsFn = defaultListLiveBackendSessionIds,
}: UseWorkspaceRestoreOptions): WorkspaceRestoreStatus {
  useEffect(() => {
    if (recoveredFromHmr) {
      setWorkspaceRestoreStatus(workspaceId, "restored");
      return;
    }

    const currentStatus = getWorkspaceRestoreStatus(workspaceId);
    if (currentStatus === "restored") return;

    let cancelled = false;
    setWorkspaceRestoreStatus(workspaceId, "loading");

    async function runRestore() {
      try {
        const session = (await loadSessionFn()) as any;
        if (cancelled) return;

        if (!session) {
          setWorkspaceRestoreStatus(workspaceId, "restored");
          return;
        }

        const liveBackendIds = await listLiveBackendSessionIdsFn();
        if (cancelled) return;

        const restoredState = deserializeWorkspaceState(workspaceId, session, liveBackendIds);
        if (cancelled) return;

        const hasRestoredTabs =
          restoredState &&
          (restoredState.layout.tabs.length > 0 ||
            Object.values(restoredState.worktreeLayouts ?? {}).some((l) => l.tabs.length > 0));

        if (hasRestoredTabs) {
          restoreWorkspace(restoredState);
          setWorkspaceRestoreStatus(workspaceId, "restored");
          return;
        }

        setWorkspaceRestoreStatus(workspaceId, "restored");
      } catch (error) {
        if (!cancelled) {
          setWorkspaceRestoreStatus(workspaceId, "failed");
          console.warn("Session restore on boot skipped:", error);
        }
      }
    }

    void runRestore();

    return () => {
      cancelled = true;
      if (getWorkspaceRestoreStatus(workspaceId) === "loading") {
        setWorkspaceRestoreStatus(workspaceId, "idle");
      }
    };
  }, [workspaceId, recoveredFromHmr, restoreWorkspace, loadSessionFn, listLiveBackendSessionIdsFn]);

  return getWorkspaceRestoreStatus(workspaceId);
}