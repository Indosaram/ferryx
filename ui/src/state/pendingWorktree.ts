import { useCallback, useRef, useState } from "react";
import type { Worktree } from "../lib/types";

export interface PendingWorktreeAgent {
  path: string;
  agentId: string;
}

/**
 * The worktree to open once a project switch lands, plus the agent chosen for it in the
 * Add Worktree dialog. Clearing the pending path also drops the agent, so a later
 * selection of the same path cannot launch a stale choice.
 */
export function usePendingWorktree() {
  const [pendingWorktree, setPendingWorktree] = useState<Worktree | null>(null);
  const pendingWorktreeAgentRef = useRef<PendingWorktreeAgent | null>(null);
  const setPendingWorktreePath = useCallback((path: string | null) => {
    if (path === null) {
      pendingWorktreeAgentRef.current = null;
      setPendingWorktree(null);
    } else {
      setPendingWorktree((current) =>
        current?.path === path
          ? current
          : { path, head: "", branch: null, bare: false, detached: false, locked: null, prunable: null },
      );
    }
  }, []);
  return { pendingWorktree, setPendingWorktree, pendingWorktreeAgentRef, setPendingWorktreePath };
}
