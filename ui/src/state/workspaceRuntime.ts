import { useCallback, useEffect, useRef, useState } from "react";

import {
  DEFAULT_WORKSPACE_ID,
  isTauriRuntime,
  listWorktrees,
  onWorktreeChanged,
  toIpcError,
} from "../lib/tauri";
import { ensureTerminalEvents } from "../lib/terminalEvents";
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

  const reportRuntimeError = useCallback((error: unknown) => {
    setRuntimeError(toIpcError(error));
  }, []);

  // Callers may build this per render; depending on the object identity would
  // re-run the initialize effect (listener re-registration + refresh) forever.
  const plainRootPath = plainRootWorktree?.path ?? null;
  const plainRootRef = useRef(plainRootWorktree);
  plainRootRef.current = plainRootWorktree;

  const refreshWorktrees = useCallback(
    (options?: { allowCreate?: boolean }) => {
      // Dedupe only within one workspace: a refresh started for the previous
      // project must never be handed to its replacement, and its result must
      // never be synced into the workspace that replaced it.
      const inFlight = refreshInFlightRef.current;
      if (inFlight && inFlight.workspaceId === workspaceId) return inFlight.promise;

      const generation = (refreshGenerationRef.current.get(workspaceId) ?? 0) + 1;
      refreshGenerationRef.current.set(workspaceId, generation);
      const isCurrent = () =>
        currentWorkspaceIdRef.current === workspaceId &&
        refreshGenerationRef.current.get(workspaceId) === generation;

      const refreshPromise = (async () => {
        try {
          const listed = await services.listWorktrees(workspaceId);
          if (!isCurrent()) return;
          let worktrees = listed;
          if (worktrees.length === 0) {
            const plainRoot = plainRootRef.current;
            if (plainRoot) {
              worktrees = [plainRoot];
            } else if (!services.isTauriRuntime()) {
              worktrees = [PREVIEW_WORKTREE];
            }
          }
          await syncWorktrees(worktrees);
          if (!isCurrent()) return;
          const preferred = worktrees.find((worktree) => worktree.path === activeWorktreePathRef.current) ?? worktrees[0];
          if (preferred) {
            if (options && options.allowCreate !== undefined) {
              await ensureTabForWorktree(preferred, options);
            } else {
              await ensureTabForWorktree(preferred);
            }
          }
          setRuntimeError(null);
        } catch (error) {
          reportRuntimeError(error);
        }
      })().finally(() => {
        if (refreshInFlightRef.current?.promise === refreshPromise) {
          refreshInFlightRef.current = null;
        }
      });

      refreshInFlightRef.current = { workspaceId, promise: refreshPromise };
      return refreshPromise;
    },
    [ensureTabForWorktree, plainRootPath, reportRuntimeError, services, syncWorktrees, workspaceId],
  );

  useEffect(() => {
    if (registeredWorkspaceId !== undefined && registeredWorkspaceId !== workspaceId) return;

    let disposed = false;
    let unlistenWorktreeChanged: (() => void) | undefined;

    const initialize = async () => {
      await services.ensureTerminalEvents();
      if (disposed) return;
      unlistenWorktreeChanged = await services.onWorktreeChanged(() => {
        void refreshWorktrees({ allowCreate: false });
      });
      if (disposed) {
        unlistenWorktreeChanged();
        return;
      }
      await refreshWorktrees();
    };

    const handleFocus = () => {
      void refreshWorktrees({ allowCreate: false });
    };

    window.addEventListener("focus", handleFocus);
    void initialize().catch(reportRuntimeError);

    return () => {
      disposed = true;
      window.removeEventListener("focus", handleFocus);
      unlistenWorktreeChanged?.();
    };
  }, [refreshWorktrees, registeredWorkspaceId, reportRuntimeError, services, workspaceId]);

  return { runtimeError, refreshWorktrees, reportRuntimeError };
}
