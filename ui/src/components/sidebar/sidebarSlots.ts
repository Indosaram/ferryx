import { combineActivitySummaries, resolveActivityIndicator, type ActivitySummary } from "../../lib/activity";
import { MACHINE_STATE_LABELS, pairedMachineStatus } from "../../lib/machineStatus";
import type { PairedHostReachabilityView } from "../../lib/pairedHostReachability";
import type { AgentState, Worktree } from "../../lib/types";
import type { RemoteHostState } from "../../state/remoteHostStore";
import type { StatusDotState } from "../ui/StatusDot";
import type { SidebarSlotState } from "./SidebarStateSlot";

export type SidebarStatusKind = "online" | "offline" | "checking" | "relay" | "failure";
/** `title` explains the status (listing error, why a machine is offline). */
export type SidebarStatus = { kind: SidebarStatusKind; label: string; title?: string };

export type AttentionState = "waiting" | "done" | "unread" | null;

/** Maps a row's activity to its slot state; idle and failed rows show nothing (ui/DESIGN.md section 4 rule 7). */
export function slotStateFor(indicator: StatusDotState | null): SidebarSlotState {
  if (indicator === "working" || indicator === "starting") return "working";
  if (indicator === "waiting" || indicator === "unread") return indicator;
  if (indicator === "done" || indicator === "exited") return "done";
  return null;
}

/** A worktree row's slot: its aggregate activity, else the bare agent state when no summary exists. */
export function worktreeRowSlot(input: {
  readonly activitySummary: ActivitySummary | undefined;
  readonly unread: boolean;
  readonly agentState: AgentState | undefined;
}): SidebarSlotState {
  const { activitySummary, unread, agentState } = input;
  const displaySummary = activitySummary
    ? activitySummary.hasUnread === unread
      ? activitySummary
      : { ...activitySummary, hasUnread: unread }
    : undefined;
  const aggregateIndicator = resolveActivityIndicator(displaySummary);
  return slotStateFor(aggregateIndicator ?? (activitySummary === undefined && agentState ? agentState : null));
}

/** Needs input outranks the supervisor; the supervisor typing outranks running and unread. */
export function supervisorSlotWins(slot: SidebarSlotState, remote: boolean, typing: boolean): boolean {
  return slot !== "waiting" && !remote && typing;
}

/** Static part of a project slot: a listing failure or the paired machine state. */
export type ProjectStatusSlot = { status: SidebarStatus; title?: string; connected: boolean };

export type PairedSlotStatus = { label: string; connected: boolean; checking: boolean; reason: string | null };

export function projectStatus(input: {
  readonly listingFailure: string | undefined;
  readonly paired: (PairedSlotStatus & { readonly staleSummary?: string }) | null;
}): ProjectStatusSlot | null {
  const { listingFailure, paired } = input;
  if (listingFailure) {
    return { status: { kind: "failure", label: "Unavailable" }, title: `Couldn't load worktrees: ${listingFailure}`, connected: false };
  }
  if (!paired) return null;
  const connected = paired.connected && !paired.staleSummary;
  const kind: SidebarStatusKind = connected ? "online" : paired.checking && !paired.staleSummary ? "checking" : "offline";
  return {
    status: { kind, label: paired.staleSummary ?? paired.label },
    ...(connected || paired.reason === null ? {} : { title: paired.reason }),
    connected,
  };
}

export type ProjectSlot =
  | { kind: "none" }
  | { kind: "failure" | "attention" | "working" | "paired"; state: SidebarSlotState; label?: string; title?: string; testId?: string; connected?: boolean };

/**
 * One status slot per project row (ui/DESIGN.md section 4 rule 7): listing failure > needs input >
 * unseen completion > running > unread > paired machine state. An unseen completion outranks live
 * work, as in resolveActivityIndicator, so a finished agent is never hidden behind a spinner.
 */
export function projectSlot(input: {
  readonly status: ProjectStatusSlot | null;
  readonly activity: ActivitySummary;
  readonly attentionState: AttentionState;
}): ProjectSlot {
  const { status, activity, attentionState } = input;
  if (status?.status.kind === "failure") {
    return { kind: "failure", state: null, label: status.status.label, title: status.title, testId: "project-worktrees-unavailable" };
  }
  if (attentionState && (attentionState === "waiting" || activity.hasDone || !activity.hasWorking)) {
    return {
      kind: "attention",
      state: attentionState,
      title: attentionState === "waiting" ? "Agent needs attention" : "Unread activity",
      testId: "project-attention-indicator",
    };
  }
  if (activity.hasWorking) {
    return {
      kind: "working",
      state: "working",
      label: activity.runningCount > 1 ? `${activity.runningCount} running` : undefined,
      testId: "project-running-badge",
    };
  }
  if (status) {
    return { kind: "paired", state: null, label: status.status.label, title: status.title, connected: status.connected };
  }
  return { kind: "none" };
}

/**
 * Paired machine text for the project slot and its worktree rows, from the shared remote status
 * vocabulary, so the header and the rows never disagree.
 */
export function pairedSlotStatus(
  hostId: string,
  state: RemoteHostState,
  reachability: PairedHostReachabilityView,
): PairedSlotStatus {
  const host = state.hosts[hostId];
  if (state.machineFeaturesEnabled === false) return { label: MACHINE_STATE_LABELS.disabled, connected: false, checking: false, reason: null };
  // Native inventory still booting: not an error yet.
  if (state.nativeStatus !== "ready") return { label: MACHINE_STATE_LABELS.checking, connected: false, checking: true, reason: null };
  if (!host) return { label: "Unavailable", connected: false, checking: false, reason: null };
  const status = pairedMachineStatus({ host, nativeReady: true, accountEnrolled: false, reachability });
  return { label: status.label, connected: status.state === "connected", checking: status.state === "checking", reason: status.reason };
}

export function projectAttentionState(activity: ActivitySummary): AttentionState {
  if (activity.hasWaiting) return "waiting";
  if (activity.hasUnread) return "unread";
  return activity.hasDone ? "done" : null;
}

const isPairedRow = (worktree: Pick<Worktree, "workspaceId">) => Boolean(worktree.workspaceId?.startsWith("daemon:"));

export function summarizeProjectActivity(
  worktrees: readonly Pick<Worktree, "path" | "workspaceId">[],
  activityByWorktreePath: Readonly<Record<string, ActivitySummary | undefined>> | undefined,
  unreadWorktreePaths: Readonly<Record<string, boolean>> | undefined,
): ActivitySummary {
  const summaries = worktrees
    .map((worktree) => isPairedRow(worktree) ? undefined : activityByWorktreePath?.[worktree.path])
    .filter((summary): summary is ActivitySummary => Boolean(summary));
  const hasUnread = worktrees.some((worktree) => !isPairedRow(worktree) && Boolean(unreadWorktreePaths?.[worktree.path]));
  return combineActivitySummaries(summaries, hasUnread);
}

/**
 * Footer aggregate (ui/DESIGN.md section 4 rule 10): each worktree counts once, needs input
 * outranking running as in the row state slot. Zero parts are omitted; both zero is empty.
 */
export function sidebarFooterSummary(activities: Iterable<ActivitySummary | undefined>): string {
  let running = 0;
  let waiting = 0;
  for (const activity of activities) {
    if (activity?.hasWaiting) waiting += 1;
    else if (activity?.hasWorking) running += 1;
  }
  return [
    running > 0 ? `${running} running` : null,
    waiting > 0 ? `${waiting} needs input` : null,
  ].filter(Boolean).join(" · ");
}
