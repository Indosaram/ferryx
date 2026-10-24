// The desktop's live-only session signals (activity summaries, bell marks and bare agent states)
// as the sidebar uses them. The desktop Sidebar feeds exactly these into sidebarLiveState, and the
// publisher sends the same object inside the snapshot, so the remote derives its rows, project
// slots and footer from the identical inputs instead of reconstructing them.
import type { ActivitySummary } from "../../lib/activity";
import type { AgentState } from "../../lib/types";
import type { SidebarModel } from "./sidebarModel";

export type SidebarSessionSignals = {
  /**
   * The desktop's own seen-aware activity per worktree path (every project, parked ones included):
   * the summaries its dots, project slots and footer are drawn from. Optional on the wire only for
   * snapshots published before this field existed; the publisher always sets it.
   */
  readonly activityByWorktreePath?: Readonly<Record<string, ActivitySummary>>;
  /** Worktree paths whose row carries a bell or unseen mark (activity's own or the store's). */
  readonly unreadWorktreePaths: Readonly<Record<string, boolean>>;
  /** Bare agent state for rows the desktop has no activity summary for (the slot's fallback). */
  readonly agentStateByPath: Readonly<Record<string, AgentState>>;
};

export const AGENT_STATES: readonly AgentState[] = ["starting", "working", "waiting", "exited", "failed"];

/**
 * Reduces the desktop's raw signals to what the shared row rule reads, for the model's rows only.
 * Paired rows never take local signals (worktreeRowLive), so they are left out.
 */
export function sidebarSessionSignals(model: SidebarModel, raw: {
  readonly activityByWorktreePath?: Readonly<Record<string, ActivitySummary | undefined>>;
  readonly unreadWorktreePaths?: Readonly<Record<string, boolean>>;
  readonly agentStateByPath?: Readonly<Record<string, AgentState | undefined>>;
}): SidebarSessionSignals {
  const activityByWorktreePath: Record<string, ActivitySummary> = {};
  const unreadWorktreePaths: Record<string, boolean> = {};
  const agentStateByPath: Record<string, AgentState> = {};
  for (const node of model.projects.flatMap((project) => project.worktrees)) {
    if (node.workspaceId.startsWith("daemon:")) continue;
    const summary = raw.activityByWorktreePath?.[node.path];
    if (summary !== undefined) activityByWorktreePath[node.path] = summary;
    if (summary?.hasUnread || raw.unreadWorktreePaths?.[node.path]) unreadWorktreePaths[node.path] = true;
    const agentState = raw.agentStateByPath?.[node.path];
    if (summary === undefined && agentState) agentStateByPath[node.path] = agentState;
  }
  return { activityByWorktreePath, unreadWorktreePaths, agentStateByPath };
}
