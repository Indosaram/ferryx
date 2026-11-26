import { isTerminalTab, type TerminalSessionSummary, type Worktree } from "../lib/types";
import type { PreSpawnedBackend, WorkspaceState } from "./workspaceStore";

export type OrphanAdoptionPlanEntry = { backend: PreSpawnedBackend; worktree: Worktree };

export type OrphanAdoptionPlan = {
  adopt: OrphanAdoptionPlanEntry[];
  /** Already shown by a tab in this window; nothing to do. */
  alreadyAttached: string[];
  /** Not in the daemon inventory any more (exited), or not running. */
  missing: string[];
};

/**
 * Decides which orphaned backend sessions (left behind by a closed secondary window) this window
 * adopts as background tabs. A session is only adopted when the daemon still lists it running and
 * no pane here already points at it; it lands in its own worktree when this workspace knows that
 * worktree, else in the active worktree. Nothing here spawns or closes a backend session.
 */
export function planOrphanAdoption(
  sessionIds: readonly string[],
  state: WorkspaceState,
  liveSessions: readonly TerminalSessionSummary[],
  /** Attach generations a source window reported, so the new pane attaches with a newer one. */
  attemptGenerations: Readonly<Record<string, number>> = {},
): OrphanAdoptionPlan {
  const plan: OrphanAdoptionPlan = { adopt: [], alreadyAttached: [], missing: [] };
  const attached = attachedBackendSessionIds(state);
  const live = new Map(liveSessions.map((summary) => [summary.sessionId, summary]));
  const activeWorktree = state.worktrees.find((worktree) => worktree.path === state.activeWorktreePath) ?? state.worktrees[0];
  for (const sessionId of new Set(sessionIds)) {
    if (attached.has(sessionId)) {
      plan.alreadyAttached.push(sessionId);
      continue;
    }
    const summary = live.get(sessionId);
    if (!summary || summary.running === false) {
      plan.missing.push(sessionId);
      continue;
    }
    const worktree = state.worktrees.find((candidate) => candidate.path === summary.worktreePath) ?? activeWorktree;
    if (!worktree) {
      plan.missing.push(sessionId);
      continue;
    }
    plan.adopt.push({
      worktree,
      backend: {
        sessionId,
        daemonEpoch: summary.daemonEpoch ?? null,
        incarnation: summary.incarnation ?? null,
        ...(attemptGenerations[sessionId] !== undefined ? { attemptGeneration: attemptGenerations[sessionId] } : {}),
      },
    });
  }
  return plan;
}

/**
 * Backend sessions some pane of this window actually shows.
 *
 * A `sessions` entry alone does not mean a pane shows the session: closing or parking a layout
 * drops the tab while the entry (and its PTY) survives, which is exactly the state a click has to
 * recover from. Counting those entries as attached made the recovery queue treat the session as
 * already on screen and return without doing anything.
 */
function attachedBackendSessionIds(state: WorkspaceState): Set<string> {
  const shown = new Set<string>();
  const layouts = [state.layout, ...Object.values(state.worktreeLayouts ?? {})];
  for (const layout of layouts) {
    for (const tab of layout?.tabs ?? []) {
      if (!isTerminalTab(tab)) continue;
      const sessionId = tab.sessionId;
      const backendSessionId = sessionId ? state.sessions[sessionId]?.backendSessionId : undefined;
      if (backendSessionId) shown.add(backendSessionId);
      for (const leafSessionId of Object.values(layout.layoutsByTabId?.[tab.id]?.sessionIdsByLeafId ?? {})) {
        const leafBackendId = leafSessionId ? state.sessions[leafSessionId]?.backendSessionId : undefined;
        if (leafBackendId) shown.add(leafBackendId);
      }
    }
  }
  return shown;
}

/**
 * Path comparison key. Mirrors `canonicalWorktreeKey` in `workspaceStore.ts`; kept local because a
 * value import of the store would drag its whole runtime (IPC, React, reducers) into every unit
 * test that only needs this planning rule.
 */
function canonicalPath(path: string | null | undefined): string {
  if (!path) return "";
  if (/^\\\\\?\\/.test(path) || /^[a-zA-Z]:[\\/]/.test(path)) {
    const stripped = path.replace(/^\\\\\?\\UNC\\/i, "\\\\").replace(/^\\\\\?\\/, "");
    return stripped.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
  }
  return path.replace(/\/+$/, "");
}

/**
 * Decides whether clicking a worktree should re-attach a session that is already running there
 * instead of spawning a second shell in the same directory. A candidate must be live, must not be
 * shown by any pane of this window already, and must belong to this exact worktree; nothing here
 * spawns or closes a backend session. Returns null when the caller should spawn as before.
 */
export function planWorktreeSessionAdoption(
  worktreePath: string,
  state: WorkspaceState,
  liveSessions: readonly TerminalSessionSummary[],
): OrphanAdoptionPlanEntry | null {
  const key = canonicalPath(worktreePath);
  if (!key) return null;
  const worktree = state.worktrees.find((candidate) => canonicalPath(candidate.path) === key);
  if (!worktree) return null;
  const attached = attachedBackendSessionIds(state);
  for (const summary of liveSessions) {
    if (summary.running === false) continue;
    if (attached.has(summary.sessionId)) continue;
    if (canonicalPath(summary.worktreePath) !== key) continue;
    return {
      worktree,
      backend: {
        sessionId: summary.sessionId,
        daemonEpoch: summary.daemonEpoch ?? null,
        incarnation: summary.incarnation ?? null,
      },
    };
  }
  return null;
}
