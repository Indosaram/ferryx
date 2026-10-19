import { invoke } from "@tauri-apps/api/core";

/**
 * Pending daemon update (todo 9). The update is installed, but the running background service
 * speaks another protocol and still owns terminals. It retires by itself when its last terminal
 * closes; ending those terminals early needs the user's confirmation of an exact count, and an
 * unknown count is never consent.
 */
export type DaemonUpdatePending = {
  runningVersion: string | null;
  /** Live sessions from control Inventory; null when they cannot be counted. */
  liveSessions: number | null;
};

export type RestartPreviousOutcome =
  | { outcome: "accepted"; live: number }
  | { outcome: "countChanged"; live: number | null }
  | { outcome: "refused"; code: string }
  | { outcome: "unavailable" };

export const DAEMON_UPDATE_PENDING = "DAEMON_UPDATE_PENDING";

/** Reads the structured details of a DAEMON_UPDATE_PENDING error; null for any other error. */
export function daemonUpdatePendingFrom(error: {
  code: string;
  details?: Record<string, unknown>;
}): DaemonUpdatePending | null {
  if (error.code !== DAEMON_UPDATE_PENDING) return null;
  const details = error.details ?? {};
  const live = details.liveSessions;
  const version = details.runningVersion;
  return {
    runningVersion: typeof version === "string" ? version : null,
    liveSessions: typeof live === "number" && Number.isInteger(live) && live >= 0 ? live : null,
  };
}

/** The restart button is enabled only for a known count. */
export function canRestartPreviousTerminals(pending: DaemonUpdatePending): boolean {
  return pending.liveSessions !== null;
}

export function pendingUpdateMessage(pending: DaemonUpdatePending): string {
  if (pending.liveSessions === null) {
    return "Update installed. The previous version cannot be managed from this version and keeps running until its terminals close.";
  }
  const n = pending.liveSessions;
  return `Update installed; ${n} ${n === 1 ? "terminal" : "terminals"} still running on the previous version`;
}

export function restartConfirmMessage(n: number): string {
  return `End ${n} running ${n === 1 ? "terminal" : "terminals"} on the previous version and restart them on this version?`;
}

/**
 * Sends control Shutdown { confirm_live_sessions: n } once. Refuses locally, with no backend call,
 * when the count is unknown.
 */
export async function restartPreviousTerminals(
  pending: DaemonUpdatePending,
): Promise<RestartPreviousOutcome> {
  if (pending.liveSessions === null) return { outcome: "unavailable" };
  return invoke<RestartPreviousOutcome>("cmd_daemon_restart_previous_terminals", {
    confirmLiveSessions: pending.liveSessions,
  });
}

/** Re-reads the pending state after the count changed. */
export async function rereadDaemonUpdatePending(): Promise<DaemonUpdatePending> {
  const status = await invoke<{ runningVersion: string | null; liveSessions: number | null }>(
    "cmd_daemon_update_pending_status",
  );
  return { runningVersion: status.runningVersion, liveSessions: status.liveSessions };
}
