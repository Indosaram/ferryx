// The attention row a desktop published inside its sidebar snapshot before the Supervisor panel
// replaced the Inbox. Desktops no longer send rows; the remote still validates them on snapshots
// from older desktops. Plus the one relative-time format the phone shell uses.

export type AttentionState = "needs-you" | "done";

export type AttentionRow = {
  id: string;
  revision: number;
  workspaceId: string;
  sessionId: string;
  state: AttentionState;
  who: string;
  location?: string;
  text?: string;
  at?: number;
  /** Set on supervisor hand-offs: the fixed stop-reason or awaiting-approval label. */
  supervisorLabel?: string;
  /** Why the row cannot be opened from here right now (shown on the row, which is then disabled). */
  unavailable?: string;
  /** What a `needs-you` row is blocked on, when the program said so. */
  kind?: AttentionBlockedKind;
};

export type AttentionBlockedKind = "permission" | "question" | "auth";

export function formatAttentionTime(at: number, now: number): string {
  const seconds = Math.max(0, Math.floor((now - at) / 1000));
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

export const ATTENTION_STATE_LABEL: Record<AttentionState, string> = {
  "needs-you": "Needs input",
  done: "Finished",
};

export const ATTENTION_STATE_SHORT_LABEL: Record<AttentionState, string> = {
  "needs-you": "Input",
  done: "Done",
};

export const ATTENTION_KIND_LABEL: Record<AttentionState, string> = {
  "needs-you": "Waiting",
  done: "Done",
};

export const ATTENTION_BLOCKED_KIND_LABEL: Record<AttentionBlockedKind, string> = {
  permission: "Permission",
  question: "Question",
  auth: "Sign-in",
};

/** The row's kind chip: the program's blocked kind when it named one, else the state's kind. */
export function attentionKindLabel(row: Pick<AttentionRow, "state" | "kind">): string {
  return row.state === "needs-you" && row.kind ? ATTENTION_BLOCKED_KIND_LABEL[row.kind] : ATTENTION_KIND_LABEL[row.state];
}


export function formatAttentionLocation(
  workspaceLabel?: string,
  worktreeLabel?: string,
): string | undefined {
  const workspace = workspaceLabel?.trim();
  const worktree = worktreeLabel?.trim();
  if (workspace && worktree) return workspace === worktree ? workspace : `${workspace} / ${worktree}`;
  return workspace || worktree || undefined;
}


export function sortAttentionRows(rows: readonly AttentionRow[]): AttentionRow[] {
  return [...rows].sort((a, b) => {
    if (a.state !== b.state) return a.state === "needs-you" ? -1 : 1;
    const timeDiff = (b.at ?? 0) - (a.at ?? 0);
    if (timeDiff !== 0) return timeDiff;
    return a.id.localeCompare(b.id);
  });
}

export function countAttentionRows(rows: readonly AttentionRow[]): Record<"all" | AttentionState, number> {
  let needsYou = 0;
  for (const row of rows) {
    if (row.state === "needs-you") needsYou += 1;
  }
  return { all: rows.length, "needs-you": needsYou, done: rows.length - needsYou };
}

export function groupAttentionRows(
  rows: readonly AttentionRow[],
): { state: AttentionState; rows: AttentionRow[] }[] {
  const groups: { state: AttentionState; rows: AttentionRow[] }[] = [];
  for (const state of ["needs-you", "done"] as const) {
    const members = rows.filter((row) => row.state === state);
    if (members.length > 0) groups.push({ state, rows: members });
  }
  return groups;
}
