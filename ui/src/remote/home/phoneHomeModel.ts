// Pure helpers the phone shell still needs after the supervisor panel became the home: the
// per-machine supervisor view the feed produces, one time format, and the lookup that turns a
// machine's session id into an openable account option. No React, no transport.
import { formatAttentionTime } from "../../features/ferryx/attention/attentionModel";
import type { RemoteSessionInventory } from "../remoteSessionInventory";
import type { RemoteSupervisorLane } from "../remoteSupervisor";
import type { SupervisorSnapshot } from "../../lib/supervisorModel";
import type { RemoteSupervisorEntry } from "../remoteSupervisor";
import type { AccountWorktreeOption } from "../useAccountWorktrees";

/** One machine's supervisor feed. `not-connected` means no tunnel is open to it. */
export type SupervisorMachineView = {
  readonly machineId: string;
  readonly label: string;
  readonly state: RemoteSupervisorEntry["state"] | "not-connected";
  readonly snapshot: SupervisorSnapshot | null;
  readonly error: { code: string; message: string } | null;
};

/** The one time format on the phone: a relative time. */
export function relativeTime(atMs: number, now: number): string {
  return formatAttentionTime(atMs, now);
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
      title: entry?.title ?? `Session in ${worktree}`,
      where: `${option.machineDisplayName} · ${option.workspaceLabel ? `${option.workspaceLabel} / ` : ""}${worktree}`,
      agentType: option.agentType ?? null,
      observedAt: entry?.activityObservedAt ?? null,
      option,
    });
  }
  const rank = (row: InboxRow) => (row.kind === "done" ? 1 : 0);
  return rows.sort((a, b) => rank(a) - rank(b) || (b.observedAt ?? 0) - (a.observedAt ?? 0));
}
