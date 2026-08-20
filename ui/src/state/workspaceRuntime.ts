import { useCallback, useEffect, useRef, useState } from "react";

import {
  isTauriRuntime,
  listWorktrees,
  onWorktreeChanged,
  toIpcError,
} from "../lib/tauri";
import { ensureTerminalEvents } from "../lib/terminalEvents";
import type { StructuredIpcError, Worktree, WorktreeChangedPayload } from "../lib/types";

export type WorkspaceRuntimeServices = {
  ensureTerminalEvents: () => Promise<void>;
  listWorktrees: () => Promise<Worktree[]>;
  onWorktreeChanged: (handler: (payload: WorktreeChangedPayload) => void) => Promise<() => void>;
  isTauriRuntime: () => boolean;
};

type UseWorkspaceRuntimeOptions = {
  activeWorktreeId: string | null;
  syncWorktrees: (worktrees: Worktree[]) => Promise<void>;
  ensureTabForWorktree: (worktree: Worktree) => Promise<string>;
  services?: WorkspaceRuntimeServices;
};

const defaultServices: WorkspaceRuntimeServices = {
  ensureTerminalEvents,
  listWorktrees,
  onWorktreeChanged,
  isTauriRuntime,
};

const PREVIEW_WORKTREE: Worktree = {
  worktreeId: "preview-main",
  wsId: "preview",
  path: ".",
  head: "main",
  branch: "refs/heads/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
  isDirty: false,
};

export function useWorkspaceRuntime({
  activeWorktreeId,
  syncWorktrees,
  ensureTabForWorktree,
  services = defaultServices,
}: UseWorkspaceRuntimeOptions) {
  const [runtimeError, setRuntimeError] = useState<StructuredIpcError | null>(null);
  const activeWorktreeIdRef = useRef(activeWorktreeId);
  const refreshInFlightRef = useRef<Promise<void> | null>(null);
  activeWorktreeIdRef.current = activeWorktreeId;

  const reportRuntimeError = useCallback((error: unknown) => {
    setRuntimeError(toIpcError(error));
  }, []);

  const refreshWorktrees = useCallback(() => {
    if (refreshInFlightRef.current) return refreshInFlightRef.current;

    const refreshPromise = (async () => {
      try {
        const listed = await services.listWorktrees();
        const worktrees = listed.length > 0 || services.isTauriRuntime() ? listed : [PREVIEW_WORKTREE];
        await syncWorktrees(worktrees);
        const preferred = worktrees.find((worktree) => worktree.worktreeId === activeWorktreeIdRef.current) ?? worktrees[0];
        if (preferred) await ensureTabForWorktree(preferred);
        setRuntimeError(null);
      } catch (error) {
        reportRuntimeError(error);
      }
    })().finally(() => {
      refreshInFlightRef.current = null;
    });

    refreshInFlightRef.current = refreshPromise;
    return refreshPromise;
  }, [ensureTabForWorktree, reportRuntimeError, services, syncWorktrees]);

  useEffect(() => {
    let disposed = false;
    let unlistenWorktreeChanged: (() => void) | undefined;

    const initialize = async () => {
      await services.ensureTerminalEvents();
      if (disposed) return;
      unlistenWorktreeChanged = await services.onWorktreeChanged(() => {
        void refreshWorktrees();
      });
      if (disposed) {
        unlistenWorktreeChanged();
        return;
      }
      await refreshWorktrees();
    };

    const handleFocus = () => {
      void refreshWorktrees();
    };

    window.addEventListener("focus", handleFocus);
    void initialize().catch(reportRuntimeError);

    return () => {
      disposed = true;
      window.removeEventListener("focus", handleFocus);
      unlistenWorktreeChanged?.();
    };
  }, [refreshWorktrees, reportRuntimeError, services]);

  return { runtimeError, refreshWorktrees, reportRuntimeError };
}
