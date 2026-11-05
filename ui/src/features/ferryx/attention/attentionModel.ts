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
