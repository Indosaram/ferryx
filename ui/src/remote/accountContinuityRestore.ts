import type { AccountContinuityPointer, AccountLastSelectedTarget } from "./accountSession";
import type { AccountWorktreeOption, UseAccountWorktreesResult } from "./useAccountWorktrees";

export type ContinuityRestoreResult =
  | { kind: "loading" | "absent" | "machine-unknown" | "machine-offline" | "grant-missing" | "session-gone" | "inventory-incomplete" | "worktree-gone" | "network-error" }
  | { kind: "session-alive"; option: AccountWorktreeOption };

type ContinuityDiscovery = Pick<UseAccountWorktreesResult, "initialized" | "loading" | "error" | "machines" | "machineStatuses" | "accountOptions" | "sessionInventories">;

export function resolveAccountContinuity(
  pointer: AccountContinuityPointer | null,
  discovery: ContinuityDiscovery,
): ContinuityRestoreResult {
  if (!pointer?.machineId || !pointer.workspaceId || !pointer.sessionId) return { kind: "absent" };
  if (discovery.error) return { kind: "network-error" };
  if (!discovery.initialized || discovery.loading) return { kind: "loading" };
  const machine = discovery.machines.find((row) => row.machineId === pointer.machineId);
  if (!machine) return { kind: "machine-unknown" };
  if (!machine.online) return { kind: "machine-offline" };
  const status = discovery.machineStatuses[machine.machineId];
  if (!status || status.status === "idle" || status.status === "tunneling") return { kind: "loading" };
  if (status.status === "offline") return { kind: "machine-offline" };
  if (status.status === "error") {
    return { kind: status.errorCode && /GRANT|UNAUTHORIZED|FORBIDDEN|ACCESS_REQUIRED/.test(status.errorCode) ? "grant-missing" : "network-error" };
  }
  const option = discovery.accountOptions.find((row) => row.machineId === pointer.machineId &&
    row.workspaceId === pointer.workspaceId && (row.worktreeSlug ?? null) === pointer.worktreeSlug);
  if (!option) return { kind: "worktree-gone" };
  const inventory = discovery.sessionInventories[machine.machineId];
  // A fetch that failed to parse, or a probe that never listed sessions, leaves no inventory.
  // That is a transport answer about the listing, not a statement about the session.
  if (!inventory) return { kind: "network-error" };
  const entry = inventory.entries.find((row) => row.sessionId === pointer.sessionId && row.machineId === pointer.machineId &&
    row.workspaceId === pointer.workspaceId && row.worktreeSlug === pointer.worktreeSlug && row.running);
  if (entry) {
    return { kind: "session-alive", option: { ...option, sessionId: entry.sessionId, daemonEpoch: entry.daemonEpoch, attachOnly: true } };
  }
  // Only a listing that claims to be complete may say a session is gone: absence in a partial
  // or unknown answer (a capped fetch, a workspace it never scanned, rows it rejected) is not
  // evidence of removal, and a restore that claimed otherwise would strand the user.
  if (inventory.completeness !== "complete") return { kind: "inventory-incomplete" };
  return { kind: "session-gone" };
}

/** The notices a refused automatic restore may publish. */
export type ContinuityRestoreFailureNotice = "session-gone" | "network-error";

/** Refusals that name this exact target as absent or restarted. Every other refusal — a 5xx,
 *  a timed-out or unparsable listing, an unconfirmed attach, a rejected pick, or a dropped
 *  fetch (no typed code at all) — is transient and leaves the attempt retryable. */
const TARGET_ABSENT_FAILURES: ReadonlySet<string> = new Set([
  "SESSION_GONE",
  "EPOCH_CHANGED",
  "SSH_NO_TERMINAL",
]);

/**
 * Classifies a refused restore attempt by its typed cause. Only a refusal that names the exact
 * session as absent, restarted, or impossible to restart in this project may claim the session
 * ended; a server error or a transport failure must stay retryable instead of telling the user
 * their session is gone.
 */
export function continuityRestoreFailureFor(
  failure: { code: string } | null,
): ContinuityRestoreFailureNotice {
  if (!failure) return "network-error";
  return TARGET_ABSENT_FAILURES.has(failure.code) ? "session-gone" : "network-error";
}

export const continuityRestoreMessages: Record<Exclude<ContinuityRestoreResult["kind"], "session-alive" | "absent">, string> = {
  loading: "Finding your last session...",
  "machine-unknown": "Your last session's computer is no longer on this account. Choose a session.",
  "machine-offline": "Your last session's computer is offline. Open Ferryx on it or choose another session.",
  "grant-missing": "Access to your last session's computer is unavailable. Reconnect it or choose another session.",
  "session-gone": "Your last session is no longer running. Choose a session; no replacement was started.",
  "inventory-incomplete": "Still checking your last session. It will open as soon as this computer finishes listing its sessions.",
  "worktree-gone": "Your last session's worktree is no longer available. Choose a session.",
  "network-error": "Couldn't resolve your last session. Check the connection or choose a session.",
};

/** The same fallback decision is used by auto-restore and by the resume card. */
export function canRestoreStoredTarget(kind: ContinuityRestoreResult["kind"]): boolean {
  return kind === "absent" || kind === "machine-unknown" || kind === "worktree-gone";
}

export function resolveWaitingLastTarget(
  pointer: AccountContinuityPointer | null,
  discovery: ContinuityDiscovery,
  stored: AccountLastSelectedTarget | null,
): (AccountLastSelectedTarget & { machineName: string; offline: boolean }) | null {
  if (!discovery.initialized || discovery.loading || discovery.error) return null;
  const { kind } = resolveAccountContinuity(pointer, discovery);
  let target: AccountLastSelectedTarget | null = null;
  if (canRestoreStoredTarget(kind)) {
    target = stored;
  } else if ((kind === "machine-offline" || kind === "network-error") && pointer?.machineId && pointer.workspaceId) {
    target = { machineId: pointer.machineId, workspaceId: pointer.workspaceId,
      worktreeSlug: pointer.worktreeSlug, sessionId: pointer.sessionId };
  }
  if (!target) return null;
  const machine = discovery.machines.find((row) => row.machineId === target.machineId);
  if (!machine) return null;
  const status = discovery.machineStatuses[target.machineId]?.status;
  const offline = !machine.online || status === "offline";
  return offline || status === "error"
    ? { ...target, machineName: machine.displayName || "Your computer", offline }
    : null;
}
