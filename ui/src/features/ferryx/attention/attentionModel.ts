import { normalizeTerminalTitle, stripLeadingActivityGlyphs } from "../../../lib/agentTitle";
import type { NotificationEntry } from "../../../lib/notificationCenter/types";
import { supervisorStopReasonLabel } from "../../../lib/supervisorModel";

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

export type AttentionActivity = { state?: string; detail?: string; seen?: boolean; kind?: AttentionBlockedKind };

export type AttentionActivityLookup = (
  workspaceId: string,
  sessionId: string,
) => AttentionActivity | undefined;

type ActivitySource = {
  workspaceId?: string | null;
  activityBySessionId?: Readonly<Record<string, AttentionActivity | undefined>>;
};

type ParkedWorkspace = readonly [workspaceId: string, state: Pick<ActivitySource, "activityBySessionId">];

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

/** The first source that names a workspace owns it, so pass the mounted state before cached snapshots. */
export function activityLookupFrom(sources: Iterable<ActivitySource | null | undefined>): AttentionActivityLookup {
  const byWorkspace = new Map<string, Readonly<Record<string, AttentionActivity | undefined>>>();
  for (const source of sources) {
    const workspaceId = source?.workspaceId;
    if (workspaceId && !byWorkspace.has(workspaceId)) byWorkspace.set(workspaceId, source.activityBySessionId ?? {});
  }
  return (workspaceId, sessionId) => byWorkspace.get(workspaceId)?.[sessionId];
}

/** The one precedence every attention surface uses: the mounted workspace, then parked snapshots. */
export function liveActivityLookup(
  mounted: ActivitySource | null | undefined,
  parked: Iterable<ParkedWorkspace>,
): AttentionActivityLookup {
  const sources: ActivitySource[] = mounted ? [mounted] : [];
  for (const [workspaceId, state] of parked) sources.push({ workspaceId, activityBySessionId: state.activityBySessionId });
  return activityLookupFrom(sources);
}

function recordedState(entry: NotificationEntry): AttentionState | null {
  if (entry.reason === "waiting") return "needs-you";
  if (entry.reason === "done") return "done";
  // Agents the detectors cannot read still ring the bell when they want the user; a plain shell's bell is not a request.
  return entry.subject === "agent" ? "needs-you" : null;
}

/**
 * The entry says an episode is unacknowledged; the session's live state, when known, says what it is now.
 * A request answered elsewhere, a completion the user already looked at, or a session that picked up new
 * work is no longer news.
 */
function attentionStateOf(entry: NotificationEntry, activity: AttentionActivity | undefined): AttentionState | null {
  if (!("unread" in entry.read)) return null;
  if (activity?.state === "working") return null;
  // A supervisor hand-off is the hub asking for the owner, whatever the pane last reported.
  if (entry.reason === "supervisor") return "needs-you";
  const live: AttentionState | null =
    activity?.state === "waiting" ? "needs-you" : activity?.state === "done" ? "done" : null;
  // A bell is not an episode the session tracks, so the seen flag belongs to some earlier wait or completion.
  if (live && activity?.seen === true && entry.reason !== "bell") return null;
  return live ?? recordedState(entry);
}

export function countAttentionEntries(
  entries: readonly NotificationEntry[],
  activityOf: AttentionActivityLookup = () => undefined,
): number {
  let count = 0;
  for (const entry of entries) {
    if (attentionStateOf(entry, activityOf(entry.workspaceId, entry.sessionId))) count += 1;
  }
  return count;
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

function displayTitle(raw: string | undefined): string | undefined {
  if (!raw) return undefined;
  return stripLeadingActivityGlyphs(normalizeTerminalTitle(raw)) || undefined;
}

export function buildAttentionRows(
  entries: readonly NotificationEntry[],
  activityOf: AttentionActivityLookup = () => undefined,
): AttentionRow[] {
  const rows: AttentionRow[] = [];
  for (const entry of entries) {
    const activity = activityOf(entry.workspaceId, entry.sessionId);
    const state = attentionStateOf(entry, activity);
    if (!state) continue;
    const title = displayTitle(entry.labels.terminalTitle);
    const who = entry.labels.agentLabel?.trim() || title || "Terminal";
    const question = activity?.state === "waiting" ? activity.detail?.trim() || undefined : undefined;
    const kind = state === "needs-you" && activity?.state === "waiting" ? activity.kind : undefined;
    const supervisorLabel = entry.reason === "supervisor" && entry.supervisor
      ? supervisorStopReasonLabel(entry.supervisor.stopReason, entry.supervisor.awaitingApproval) ?? undefined
      : undefined;
    rows.push({
      id: entry.id,
      revision: entry.revision,
      workspaceId: entry.workspaceId,
      sessionId: entry.sessionId,
      state,
      who,
      location: formatAttentionLocation(entry.labels.workspaceLabel, entry.labels.worktreeLabel),
      text: question ?? (title !== who ? title : undefined),
      at: entry.lastOccurredAt,
      ...(supervisorLabel ? { supervisorLabel } : {}),
      ...(kind ? { kind } : {}),
    });
  }
  return sortAttentionRows(rows);
}

/**
 * Unread agent entries whose pane reads working. The detector can miss a prompt (an agent menu
 * the screen rules do not know), so the Inbox asks the daemon about each and lists only those
 * whose pane really holds a prompt, as needs-you. They are not rows on their own.
 */
export function buildPromptCandidateRows(
  entries: readonly NotificationEntry[],
  activityOf: AttentionActivityLookup = () => undefined,
): AttentionRow[] {
  const rows: AttentionRow[] = [];
  for (const entry of entries) {
    if (!("unread" in entry.read) || entry.reason === "supervisor") continue;
    if (activityOf(entry.workspaceId, entry.sessionId)?.state !== "working") continue;
    const title = displayTitle(entry.labels.terminalTitle);
    const who = entry.labels.agentLabel?.trim() || title || "Terminal";
    rows.push({
      id: entry.id,
      revision: entry.revision,
      workspaceId: entry.workspaceId,
      sessionId: entry.sessionId,
      state: "needs-you",
      who,
      location: formatAttentionLocation(entry.labels.workspaceLabel, entry.labels.worktreeLabel),
      text: title !== who ? title : undefined,
      at: entry.lastOccurredAt,
    });
  }
  return rows;
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

export type RemotePaneActivity = {
  id: string;
  label: string;
  activityState?: "working" | "waiting" | "done";
  agentType?: string;
  worktreeLabel?: string;
};

/**
 * The phone only learns each pane's activity, and the desktop already stops sending a state the
 * user has looked at, so a pane that is waiting or done here is unacknowledged by definition.
 */
export function buildRemoteAttentionRows(
  workspaceId: string,
  workspaceLabel: string | null | undefined,
  panes: readonly RemotePaneActivity[],
  activePaneId?: string | null,
): AttentionRow[] {
  const rows: AttentionRow[] = [];
  for (const pane of panes) {
    if (pane.id === activePaneId) continue;
    const state: AttentionState | null =
      pane.activityState === "waiting" ? "needs-you" : pane.activityState === "done" ? "done" : null;
    if (!state) continue;
    const agent = pane.agentType?.trim();
    rows.push({
      id: pane.id,
      revision: 0,
      workspaceId,
      sessionId: pane.id,
      state,
      who: agent || pane.label,
      location: formatAttentionLocation(workspaceLabel ?? workspaceId, pane.worktreeLabel),
      text: agent && pane.label !== agent ? pane.label : undefined,
    });
  }
  return sortAttentionRows(rows);
}

export function formatAttentionTime(at: number, now: number): string {
  const seconds = Math.max(0, Math.floor((now - at) / 1000));
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}
