// Pure derivations for the phone home: the supervisor headline across machines, lane status,
// the cross-machine Inbox, and the merged audit list. No React, no transport.
import { actingSessionIds, type SupervisorAction, type SupervisorLane, type SupervisorSnapshot } from "../../lib/supervisorModel";
import { setupNeedsAttention } from "../../lib/supervisorSetupModel";
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

export type SupervisorHeadline = "off" | "needs_setup" | "paused" | "typing" | `watching ${number}`;

/** The first readable machine whose supervisor setup needs the person, or null. */
export function setupMachine(machines: readonly SupervisorMachineView[]): SupervisorMachineView | null {
  return machines.find((m) => setupNeedsAttention(m.snapshot?.setup ?? null)) ?? null;
}

/**
 * The SupervisorPanelView headline, over every machine whose snapshot is readable:
 * off > needs setup > paused > typing > watching. A machine that does not report setup
 * (older daemon) never reads as needing it.
 */
export function supervisorHeadline(machines: readonly SupervisorMachineView[], now: number): SupervisorHeadline {
  const snapshots = machines.flatMap((m) => (m.snapshot ? [m.snapshot] : []));
  const lanes = snapshots.flatMap((s) => s.lanes);
  if (!snapshots.some((s) => s.hub.running)) return "off";
  if (setupMachine(machines)) return "needs_setup";
  if (lanes.length > 0 && lanes.every((lane) => lane.paused)) return "paused";
  if (snapshots.some((s) => {
    const acting = actingSessionIds(s, now);
    return s.lanes.some((lane) => lane.sessionId !== null && acting.has(lane.sessionId));
  })) return "typing";
  const watched = snapshots.reduce((sum, s) => sum + s.watched.length, 0);
  return `watching ${lanes.length + watched}`;
}

/** `machine` names the computer for "needs_setup"; the other headlines ignore it. */
export function headlineSentence(headline: SupervisorHeadline, machine?: string): string {
  if (headline === "off") return "Nothing is typed into your sessions. Start it on a computer with ferryx hub start.";
  if (headline === "needs_setup") return `Supervisor setup isn't finished on ${machine || "your computer"}`;
  if (headline === "paused") return "Paused. It still reads your sessions but sends nothing until you resume.";
  if (headline === "typing") return "It is typing in a lane right now. Open the lane to take over.";
  return "Acts only in the lanes below and holds input while you type.";
}

export function laneName(lane: Pick<SupervisorLane, "cwd" | "title">): string {
  const leaf = lane.cwd.split(/[\\/]/).filter(Boolean).pop();
  return leaf ? `${leaf} · ${lane.title}` : lane.title;
}

export type LaneStatus = { readonly kind: "paused" | "typing" | "needs-you" | "status"; readonly label: string };

/** One status slot per lane: paused > typing > needs you > the hub's own status. */
export function laneStatus(lane: SupervisorLane, acting: ReadonlySet<string>, needsYou: boolean): LaneStatus {
  if (lane.paused) return { kind: "paused", label: "Paused" };
  if (lane.sessionId && acting.has(lane.sessionId)) return { kind: "typing", label: "Typing" };
  if (needsYou) return { kind: "needs-you", label: "Needs you" };
  const text = lane.status.replace(/_/g, " ").trim();
  return { kind: "status", label: text ? text[0].toUpperCase() + text.slice(1) : "Idle" };
}

export function clock(atMs: number): string {
  const date = new Date(atMs);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

export function relativeTime(atMs: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - atMs) / 1000));
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
}

export type RecentAction = SupervisorAction & { readonly machineId: string; readonly where: string };

/** Every machine's audit entries, newest first, capped at `limit`. */
export function recentActions(machines: readonly SupervisorMachineView[], limit = 20): RecentAction[] {
  const merged: RecentAction[] = [];
  for (const machine of machines) {
    const snapshot = machine.snapshot;
    if (!snapshot) continue;
    for (const action of snapshot.actions) {
      const lane = snapshot.lanes.find((item) => item.sessionId === action.sessionId);
      merged.push({ ...action, machineId: machine.machineId, where: lane ? lane.title : (action.sessionId ?? "pane").slice(0, 8) });
    }
  }
  return merged.sort((a, b) => b.atMs - a.atMs).slice(0, limit);
}

export function actionText(action: RecentAction): string {
  if (action.code === "CONTROL_CONFLICT") return `${action.where} · held, you were typing`;
  if (action.code !== "OK") return `${action.where} · refused (${action.code.toLowerCase()})`;
  return action.text ? `${action.where} · “${action.text}”` : action.where;
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
  held: "Supervisor paused this lane",
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
