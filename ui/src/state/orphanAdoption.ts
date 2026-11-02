import type { TerminalSessionSummary, Worktree } from "../lib/types";
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
  const attached = new Set(
    Object.values(state.sessions)
      .map((session) => session.backendSessionId)
      .filter((id): id is string => typeof id === "string" && id.length > 0),
  );
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
