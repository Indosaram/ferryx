// The desktop's live-only session signals (activity summaries, bell marks and bare agent states)
// as the sidebar uses them. The desktop Sidebar feeds exactly these into sidebarLiveState, and the
// publisher sends the same object inside the snapshot, so the remote derives its rows, project
// slots and footer from the identical inputs instead of reconstructing them.
import type { ActivitySummary } from "../../lib/activity";
import type { AttentionRow } from "../../features/ferryx/attention/attentionModel";
import { worktreeIdentity, type AgentState, type TerminalSession, type Worktree } from "../../lib/types";
import type { SidebarModel } from "./sidebarModel";

/** At most this many Inbox rows ride in one snapshot (the desktop's first, most urgent rows). */
export const PUBLISHED_ATTENTION_ROWS_CAP = 50;

/**
 * A desktop Inbox row as published. `sessionId` is the desktop's local pane id, which means nothing
 * to the remote; `backendSessionId` is that pane's PTY id (the id the machine's session inventory
 * lists) and `worktreeSlug` its worktree (null for the root). Both are absent when the desktop no
 * longer holds the pane, and on snapshots published before they existed.
 */
export type PublishedAttentionRow = AttentionRow & {
  readonly backendSessionId?: string;
  readonly worktreeSlug?: string | null;
};

/** One workspace's sessions and worktrees, as the mounted state and the parked snapshots hold them. */
export type AttentionSessionSource = readonly [
  workspaceId: string | null | undefined,
  state: { readonly sessions: Readonly<Record<string, TerminalSession | undefined>>; readonly worktrees: readonly Worktree[] },
];

/**
 * Adds each row's backend session id and worktree slug, read from the workspace that owns the row's
 * local session (the first source naming a workspace owns it, so pass the mounted state first).
 */
export function withBackendSessions(
  rows: readonly AttentionRow[],
  sources: Iterable<AttentionSessionSource>,
): PublishedAttentionRow[] {
  const byWorkspace = new Map<string, AttentionSessionSource[1]>();
  for (const [workspaceId, state] of sources) {
    if (workspaceId && !byWorkspace.has(workspaceId)) byWorkspace.set(workspaceId, state);
  }
  return rows.map((row) => {
    const state = byWorkspace.get(row.workspaceId);
    const session = state?.sessions[row.sessionId];
    if (!state || !session) return row;
    const path = session.worktreePath ?? session.cwd;
    const worktree = state.worktrees.find((candidate) => candidate.path === path);
    return {
      ...row,
      ...(session.backendSessionId ? { backendSessionId: session.backendSessionId } : {}),
      ...(worktree ? { worktreeSlug: worktreeIdentity(worktree)?.slug ?? null } : {}),
    };
  });
}

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
  /**
   * The desktop Inbox's rows (its notification centre), in its order and capped. The remote lists
   * exactly these and counts them, so its Inbox tab and list agree with each other and with the
   * desktop. Absent from older snapshots.
   */
  readonly attentionRows?: readonly PublishedAttentionRow[];
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
  readonly attentionRows?: readonly PublishedAttentionRow[];
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
  return {
    activityByWorktreePath,
    unreadWorktreePaths,
    agentStateByPath,
    ...(raw.attentionRows === undefined ? {} : { attentionRows: raw.attentionRows.slice(0, PUBLISHED_ATTENTION_ROWS_CAP) }),
  };
}
