import { useCallback, useEffect, useRef, useState } from "react";

import {
  DEFAULT_WORKSPACE_ID,
  isStructuredIpcError,
  isTauriRuntime,
  listWorktrees,
  onWorktreeChanged,
  toIpcError,
} from "../lib/tauri";
import { ensureTerminalEvents } from "../lib/terminalEvents";
import { switchDebug } from "../lib/switchDebug";
import type { StructuredIpcError, Worktree, WorktreeChangedPayload } from "../lib/types";

export type WorkspaceRuntimeServices = {
  ensureTerminalEvents: () => Promise<void>;
  listWorktrees: (workspaceId: string) => Promise<Worktree[]>;
  onWorktreeChanged: (handler: (payload: WorktreeChangedPayload) => void) => Promise<() => void>;
  isTauriRuntime: () => boolean;
};

type UseWorkspaceRuntimeOptions = {
  workspaceId?: string;
  activeWorktreePath: string | null;
  syncWorktrees: (worktrees: Worktree[]) => Promise<void>;
  ensureTabForWorktree: (worktree: Worktree, options?: { allowCreate?: boolean }) => Promise<string | null>;
  /// Synthesized primary "worktree" for plain (non-Git) workspaces whose root
  /// is the project folder itself. Used when the backend lists no worktrees.
  plainRootWorktree?: Worktree | null;
  /// Workspace ID the backend registry has accepted. Listing worktrees before
  /// registration completes fails with WORKSPACE_NOT_FOUND and leaves the
  /// sidebar empty, so syncing waits for this to match `workspaceId`.
  registeredWorkspaceId?: string | null;
  services?: WorkspaceRuntimeServices;
};

const defaultServices: WorkspaceRuntimeServices = {
  ensureTerminalEvents,
  listWorktrees,
  onWorktreeChanged,
  isTauriRuntime,
};

const PREVIEW_WORKTREE: Worktree = {
  path: ".",
  head: "main",
  branch: "refs/heads/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function extractErrorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  if (isStructuredIpcError(error)) return error.message;
  if (error && typeof error === "object") {
    if ("message" in error && typeof (error as { message?: unknown }).message === "string") {
      return (error as { message: string }).message;
    }
    try {
      return JSON.stringify(error);
    } catch {
      return String(error);
    }
  }
  return String(error);
}

function sanitizeDebugDetails(details: Record<string, unknown>): Record<string, unknown> {
  const sanitized: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(details)) {
    if (typeof value === "string") {
      sanitized[key] = value.length > 500 ? `${value.slice(0, 500)}...` : value;
    } else {
      try {
        JSON.stringify(value);
        sanitized[key] = value;
      } catch {
        const str = String(value);
        sanitized[key] = str.length > 500 ? `${str.slice(0, 500)}...` : str;
      }
    }
  }
  return sanitized;
}

export function useWorkspaceRuntime({
  workspaceId = DEFAULT_WORKSPACE_ID,
  activeWorktreePath,
  syncWorktrees,
  ensureTabForWorktree,
  plainRootWorktree = null,
  registeredWorkspaceId,
  services = defaultServices,
}: UseWorkspaceRuntimeOptions) {
  const [runtimeError, setRuntimeError] = useState<StructuredIpcError | null>(null);
  const activeWorktreePathRef = useRef(activeWorktreePath);
  const refreshInFlightRef = useRef<{ workspaceId: string; promise: Promise<void> } | null>(null);
  // A->B->A can leave an older refresh for the same workspace still pending;
  // only the newest generation per workspace may sync its result.
  const refreshGenerationRef = useRef(new Map<string, number>());
  const currentWorkspaceIdRef = useRef(workspaceId);
  currentWorkspaceIdRef.current = workspaceId;
  activeWorktreePathRef.current = activeWorktreePath;

  const syncWorktreesRef = useRef(syncWorktrees);
  syncWorktreesRef.current = syncWorktrees;
  const ensureTabForWorktreeRef = useRef(ensureTabForWorktree);
  ensureTabForWorktreeRef.current = ensureTabForWorktree;
  const servicesRef = useRef(services);
  servicesRef.current = services;

  const reportRuntimeError = useCallback((error: unknown) => {
    const ipcError = toIpcError(error);
    const sanitizedDetails = sanitizeDebugDetails(ipcError.details ?? {});
    switchDebug("workspace.runtime.error", {
      workspaceId: currentWorkspaceIdRef.current,
      error: extractErrorMessage(error),
      code: ipcError.code,
      ...sanitizedDetails,
    });
    setRuntimeError({ ...ipcError });
  }, []);
  const reportRuntimeErrorRef = useRef(reportRuntimeError);
  reportRuntimeErrorRef.current = reportRuntimeError;

  // Callers may build this per render; depending on the object identity would
  // re-run the initialize effect (listener re-registration + refresh) forever.
  const plainRootRef = useRef(plainRootWorktree);
  plainRootRef.current = plainRootWorktree;

  const refreshWorktrees = useCallback(
    (options?: { allowCreate?: boolean }) => {
      // Dedupe only within one workspace: a refresh started for the previous
      // project must never be handed to its replacement, and its result must
      // never be synced into the workspace that replaced it.
      const inFlight = refreshInFlightRef.current;
      if (inFlight && inFlight.workspaceId === workspaceId) {
        switchDebug("worktree.refresh.deduped", { workspaceId });
        return inFlight.promise;
      }

      const generation = (refreshGenerationRef.current.get(workspaceId) ?? 0) + 1;
      refreshGenerationRef.current.set(workspaceId, generation);
      const isCurrent = () =>
        currentWorkspaceIdRef.current === workspaceId &&
        refreshGenerationRef.current.get(workspaceId) === generation;

      const refreshPromise = (async () => {
        try {
          switchDebug("worktree.refresh.start", {
            workspaceId,
            generation,
            allowCreate: options?.allowCreate ?? true,
            activeWorktreePath: activeWorktreePathRef.current,
          });
          const listed = await servicesRef.current.listWorktrees(workspaceId);
          switchDebug("worktree.refresh.listed", {
            workspaceId,
            generation,
            listedCount: listed.length,
            listedPaths: listed.map((worktree) => worktree.path),
            current: isCurrent(),
          });
          if (!isCurrent()) return;
          let worktrees = listed;
          if (worktrees.length === 0) {
            const plainRoot = plainRootRef.current;
            if (plainRoot) {
              worktrees = [plainRoot];
            } else if (!servicesRef.current.isTauriRuntime()) {
              worktrees = [PREVIEW_WORKTREE];
            }
          }
          switchDebug("worktree.refresh.sync.start", {
            workspaceId,
            generation,
            worktreeCount: worktrees.length,
          });
          await syncWorktreesRef.current(worktrees);
          switchDebug("worktree.refresh.sync.complete", {
            workspaceId,
            generation,
            current: isCurrent(),
          });
          if (!isCurrent()) return;
          const preferred = worktrees.find((worktree) => worktree.path === activeWorktreePathRef.current) ?? worktrees[0];
          if (preferred) {
            switchDebug("worktree.refresh.ensure.start", {
              workspaceId,
              generation,
              preferredPath: preferred.path,
              allowCreate: options?.allowCreate ?? true,
            });
            if (options && options.allowCreate !== undefined) {
              await ensureTabForWorktreeRef.current(preferred, options);
            } else {
              await ensureTabForWorktreeRef.current(preferred);
            }
          }
          switchDebug("worktree.refresh.complete", {
            workspaceId,
            generation,
            preferredPath: preferred?.path ?? null,
          });
          setRuntimeError(null);
        } catch (error) {
          reportRuntimeErrorRef.current(error);
        }
      })().finally(() => {
        if (refreshInFlightRef.current?.promise === refreshPromise) {
          refreshInFlightRef.current = null;
        }
      });

      refreshInFlightRef.current = { workspaceId, promise: refreshPromise };
      return refreshPromise;
    },
    [workspaceId],
  );

  const refreshWorktreesRef = useRef(refreshWorktrees);
  refreshWorktreesRef.current = refreshWorktrees;

  useEffect(() => {
    if (registeredWorkspaceId !== undefined && registeredWorkspaceId !== workspaceId) {
      switchDebug("workspace.runtime.gated", {
        workspaceId,
        registeredWorkspaceId: registeredWorkspaceId ?? null,
      });
      return;
    }

    let disposed = false;
    let unlistenWorktreeChanged: (() => void) | undefined;

    const initialize = async () => {
      switchDebug("workspace.runtime.initialize.start", { workspaceId });
      await servicesRef.current.ensureTerminalEvents();
      if (disposed) return;
      unlistenWorktreeChanged = await servicesRef.current.onWorktreeChanged(() => {
        switchDebug("worktree.changed.event", { workspaceId });
        void refreshWorktreesRef.current({ allowCreate: false });
      });
      if (disposed) {
        unlistenWorktreeChanged();
        return;
      }
      await refreshWorktreesRef.current();
      switchDebug("workspace.runtime.initialize.complete", { workspaceId });
    };

    const handleFocus = () => {
      void refreshWorktreesRef.current({ allowCreate: false });
    };

    window.addEventListener("focus", handleFocus);
    void initialize().catch((err) => reportRuntimeErrorRef.current(err));

    return () => {
      disposed = true;
      switchDebug("workspace.runtime.dispose", { workspaceId });
      window.removeEventListener("focus", handleFocus);
      unlistenWorktreeChanged?.();
    };
  }, [registeredWorkspaceId, workspaceId]);

  return { runtimeError, refreshWorktrees, reportRuntimeError };
}
