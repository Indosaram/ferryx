// Pure helpers the phone shell still needs after the supervisor panel became the home: the
// per-machine supervisor view the feed produces, one time format, and the lookup that turns a
// machine's session id into an openable account option. No React, no transport.
import { formatAttentionTime } from "../../features/ferryx/attention/attentionModel";
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
