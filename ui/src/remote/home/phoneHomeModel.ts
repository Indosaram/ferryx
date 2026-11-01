// Pure derivations for the phone home: the supervisor headline across machines, lane status,
// the cross-machine Inbox, and the merged audit list. No React, no transport.
import { formatAttentionTime } from "../../features/ferryx/attention/attentionModel";
import { actingSessionIds, type SupervisorAction, type SupervisorLane, type SupervisorSnapshot } from "../../lib/supervisorModel";
import type { RemoteSupervisorEntry, RemoteSupervisorLane } from "../remoteSupervisor";
import type { RemoteSessionInventory } from "../remoteSessionInventory";
import type { AccountWorktreeOption } from "../useAccountWorktrees";

/** One machine section on the Supervisor home. `not-connected` means no tunnel is open to it. */
export type SupervisorMachineView = {
  readonly machineId: string;
  readonly label: string;
  readonly state: RemoteSupervisorEntry["state"] | "not-connected";
  readonly snapshot: SupervisorSnapshot | null;
  readonly error: { code: string; message: string } | null;
};

/** What the top of the Supervisor home says. `unknown` means no computer could be read. */
export type SupervisorHeadline =
  | { readonly kind: "unknown" }
  | { readonly kind: "off" }
  | { readonly kind: "paused"; readonly lanes: number }
  | { readonly kind: "typing"; readonly lanes: number }
  | { readonly kind: "active"; readonly lanes: number; readonly watched: number };

/** The SupervisorPanelView headline, over every machine whose snapshot is readable. */
export function supervisorHeadline(machines: readonly SupervisorMachineView[], now: number): SupervisorHeadline {
  const snapshots = machines.flatMap((m) => (m.snapshot ? [m.snapshot] : []));
  if (snapshots.length === 0) return { kind: "unknown" };
  const lanes = snapshots.flatMap((s) => s.lanes);
  if (!snapshots.some((s) => s.hub.running)) return { kind: "off" };
  if (lanes.length > 0 && lanes.every((lane) => lane.paused)) return { kind: "paused", lanes: lanes.length };
  if (snapshots.some((s) => {
    const acting = actingSessionIds(s, now);
    return s.lanes.some((lane) => lane.sessionId !== null && acting.has(lane.sessionId));
  })) return { kind: "typing", lanes: lanes.length };
  const watched = snapshots.reduce((sum, s) => sum + s.watched.length, 0);
  return { kind: "active", lanes: lanes.length, watched };
}

function plural(count: number, word: string): string {
  return `${count} ${word}${count === 1 ? "" : "s"}`;
}

export function headlineSentence(headline: SupervisorHeadline): string {
  switch (headline.kind) {
    case "unknown":
      return "Can't reach the supervisor on your computers right now.";
    case "off":
      return "The supervisor isn't running on any of your computers.";
    case "paused":
      return "Paused on every lane. It still reads your sessions but sends nothing.";
    case "typing":
      return "Typing in a lane right now. Open the lane to take over.";
    case "active": {
      if (headline.lanes === 0 && headline.watched === 0) return "Running, with no lanes yet.";
      const watched = headline.watched > 0 ? `, watching ${plural(headline.watched, "more session")}` : "";
      return `Running ${plural(headline.lanes, "lane")}${watched}.`;
    }
    default: {
      const unreachable: never = headline;
      return unreachable;
    }
  }
}

/** The folder a lane works in, shown as secondary meta under its title. */
export function laneFolder(lane: Pick<SupervisorLane, "cwd">): string | null {
  return lane.cwd.split(/[\\/]/).filter(Boolean).pop() ?? null;
}

export type LaneStatus = { readonly kind: "paused" | "typing" | "needs-you" | "status"; readonly label: string };

const LANE_STATUS_LABEL: Readonly<Record<string, string>> = {
  running: "Working",
  working: "Working",
  active: "Working",
  idle: "Idle",
  waiting: "Waiting",
  waiting_on_review: "Waiting for review",
  waiting_on_user: "Needs you",
  blocked: "Blocked",
  blocked_on_ci: "Waiting for checks",
  done: "Done",
  completed: "Done",
  failed: "Failed",
  stopped: "Stopped",
};

/** One status slot per lane: paused > typing > needs you > the hub's status, from a fixed label set. */
export function laneStatus(lane: SupervisorLane, acting: ReadonlySet<string>, needsYou: boolean): LaneStatus {
  if (lane.paused) return { kind: "paused", label: "Paused" };
  if (lane.sessionId && acting.has(lane.sessionId)) return { kind: "typing", label: "Typing" };
  if (needsYou) return { kind: "needs-you", label: "Needs you" };
  return { kind: "status", label: LANE_STATUS_LABEL[lane.status.trim().toLowerCase()] ?? "Working" };
}

/** The home's one time format: the Inbox's relative time. */
export function relativeTime(atMs: number, now: number): string {
  return formatAttentionTime(atMs, now);
}

export type RecentAction = SupervisorAction & {
  readonly machineId: string;
  readonly machineLabel: string;
  /** The lane title, or null when the session belongs to no lane. */
  readonly where: string | null;
};

/** Every machine's audit entries, newest first, capped at `limit`. */
export function recentActions(machines: readonly SupervisorMachineView[], limit = 20): RecentAction[] {
  const merged: RecentAction[] = [];
  for (const machine of machines) {
    const snapshot = machine.snapshot;
    if (!snapshot) continue;
    for (const action of snapshot.actions) {
      const lane = snapshot.lanes.find((item) => item.sessionId === action.sessionId);
      merged.push({ ...action, machineId: machine.machineId, machineLabel: machine.label, where: lane ? lane.title : null });
    }
  }
  return merged.sort((a, b) => b.atMs - a.atMs).slice(0, limit);
}

export function actionText(action: RecentAction): string {
  if (action.code === "CONTROL_CONFLICT") return "Held off while you were typing";
  if (action.code !== "OK") return "Not sent";
  return action.text ? `Typed \u201c${action.text}\u201d` : "Typed into the session";
}

export type InboxRowKind = "waiting" | "held" | "done";

export type InboxRow = {
  readonly key: string;
  readonly kind: InboxRowKind;
  readonly machineId: string;
  readonly sessionId: string;
  readonly title: string;
  readonly where: string;
  readonly agentType: string | null;
  readonly observedAt: number | null;
  readonly option: AccountWorktreeOption;
};

export const INBOX_WHY: Record<InboxRowKind, string> = {
  waiting: "Waiting for your answer",
  held: "Supervisor paused",
  done: "Finished",
};

/**
 * Needs-you sessions across every machine: waiting (and supervisor-held waiting) first, then
 * finished; newest observation first inside each group. Built from the account options, so a row
 * is exactly as openable as the matching worktree row.
 */
export function buildInboxRows(input: {
  readonly options: readonly AccountWorktreeOption[];
  readonly inventories: Readonly<Record<string, RemoteSessionInventory>>;
  readonly lanes: readonly RemoteSupervisorLane[];
}): InboxRow[] {
  const rows: InboxRow[] = [];
  for (const option of input.options) {
    const state = option.inventoryActivityState;
    if (!option.sessionId || (state !== "waiting" && state !== "done")) continue;
    const entry = input.inventories[option.machineId]?.entries.find((item) => item.sessionId === option.sessionId);
    const held = state === "waiting" && input.lanes.some(
      (lane) => lane.machineId === option.machineId && lane.sessionId === option.sessionId && lane.paused,
    );
    const worktree = option.worktreeLabel ?? option.worktreeSlug ?? "main";
    rows.push({
      key: `${option.machineId}\u0000${option.sessionId}`,
      kind: state === "done" ? "done" : held ? "held" : "waiting",
      machineId: option.machineId,
      sessionId: option.sessionId,
      title: entry?.title ?? `Session ${option.sessionId.slice(0, 8)}`,
      where: `${option.machineDisplayName} · ${option.workspaceLabel ? `${option.workspaceLabel} / ` : ""}${worktree}`,
      agentType: option.agentType ?? null,
      observedAt: entry?.activityObservedAt ?? null,
      option,
    });
  }
  const rank = (row: InboxRow) => (row.kind === "done" ? 1 : 0);
  return rows.sort((a, b) => rank(a) - rank(b) || (b.observedAt ?? 0) - (a.observedAt ?? 0));
}

/** The account option that opens one machine's session, or null when it is not openable now. */
export function optionForSession(
  options: readonly AccountWorktreeOption[],
  machineId: string,
  sessionId: string | null,
): AccountWorktreeOption | null {
  if (!sessionId) return null;
  return options.find((option) => option.machineId === machineId && option.sessionId === sessionId && !option.unavailable) ?? null;
}
