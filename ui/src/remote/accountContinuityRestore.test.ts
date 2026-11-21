import { describe, expect, it } from "vitest";
import { continuityRestoreFailureFor, resolveAccountContinuity, resolveWaitingLastTarget } from "./accountContinuityRestore";
import type { AccountContinuityPointer, AccountMachineView } from "./accountSession";
import type { UseAccountWorktreesResult } from "./useAccountWorktrees";
import { parseSessionsPayload } from "./remoteSessionInventory";

const pointer: AccountContinuityPointer = {
  machineId: "machine", workspaceId: "workspace", worktreeSlug: null,
  sessionId: "exact", revision: 4, updatedAt: 10,
};
const machine: AccountMachineView = {
  machineRecordId: "record", machineId: "machine", displayName: "Computer", publicKey: "key",
  attachPublicKey: "attach", relayOrigin: "https://relay.test", platform: "macos",
  online: true, enrollmentEpoch: "1", lastSeenAt: 10,
};

function discovery() {
  const parsed = parseSessionsPayload("machine", {
    completeness: "complete", sessions: ["other", "exact"].map((sessionId) => ({
      sessionId, workspaceId: "workspace", worktree: null, running: true, daemonEpoch: "epoch",
    })),
  });
  if (!parsed.ok) throw new Error("Invalid test inventory");
  return {
    initialized: true, loading: false, error: null,
    machines: [machine],
    machineStatuses: { machine: { machine, status: "ready", options: [] } },
    accountOptions: [{ machineId: "machine", workspaceId: "workspace", worktreeSlug: null, worktreeLabel: null,
      machineDisplayName: "Computer", machinePlatform: "macos", machineOnline: true }],
    sessionInventories: { machine: parsed.inventory },
  } satisfies Pick<UseAccountWorktreesResult, "initialized" | "loading" | "error" | "machines" | "machineStatuses" | "accountOptions" | "sessionInventories">;
}

describe("home resume target", () => {
  const localMachine = { ...machine, machineId: "device-local", displayName: "Laptop", online: false };
  const local = { machineId: localMachine.machineId, workspaceId: "local-workspace", worktreeLabel: "Local" };
  const both = () => ({ ...discovery(), machines: [machine, localMachine] });

  it("waits for the device target when the account worktree disappeared", () => {
    const state = { ...both(), accountOptions: [] };
    expect(resolveWaitingLastTarget(pointer, state, local)).toMatchObject({ ...local, machineName: "Laptop", offline: true });
  });

  it("never substitutes an offline device target for a confirmed missing exact session", () => {
    expect(resolveWaitingLastTarget({ ...pointer, sessionId: "ended" }, both(), local)).toBeNull();
  });

  it("shows the exact offline account machine ahead of the stored device", () => {
    const state = { ...both(), machines: [{ ...machine, online: false }, localMachine] };
    expect(resolveWaitingLastTarget(pointer, state, local)).toMatchObject({ machineId: machine.machineId, sessionId: "exact", offline: true });
  });

  it("does not announce the old target while refreshing discovery", () => {
    expect(resolveWaitingLastTarget(null, { ...both(), loading: true }, local)).toBeNull();
  });

  it("distinguishes a failed probe from an offline computer", () => {
    const state = { ...both(), machineStatuses: { machine: { machine, status: "error" as const, options: [] } } };
    expect(resolveWaitingLastTarget(pointer, state, local)).toMatchObject({ machineId: machine.machineId, offline: false });
  });
});

describe("account exact-session continuity resolution", () => {
  it("resolves the exact session, never the first sibling", () => {
    const result = resolveAccountContinuity(pointer, discovery());
    expect(result.kind).toBe("session-alive");
    if (result.kind === "session-alive") expect(result.option).toMatchObject({ sessionId: "exact", daemonEpoch: "epoch", attachOnly: true });
  });

  it("does not replace a missing or stopped exact session in a complete listing", () => {
    const state = discovery();
    state.sessionInventories.machine.entries = state.sessionInventories.machine.entries.filter((row) => row.sessionId !== "exact");
    expect(resolveAccountContinuity(pointer, state).kind).toBe("session-gone");
    expect(resolveAccountContinuity({ ...pointer, sessionId: "missing" }, discovery()).kind).toBe("session-gone");
    const stopped = discovery();
    stopped.sessionInventories.machine.entries = stopped.sessionInventories.machine.entries.map((row) =>
      row.sessionId === "exact" ? { ...row, running: false } : row);
    expect(resolveAccountContinuity(pointer, stopped).kind).toBe("session-gone");
  });

  it.each(["partial", "unknown"] as const)("never claims a session is gone from a %s listing", (completeness) => {
    // Absence in a capped fetch, an unscanned workspace, or rows the machine rejected is not
    // evidence of removal. Claiming it is gone would strand the one target the user can resume.
    const state = discovery();
    state.sessionInventories.machine.entries = state.sessionInventories.machine.entries.filter((row) => row.sessionId !== "exact");
    state.sessionInventories.machine.completeness = completeness;
    expect(resolveAccountContinuity(pointer, state).kind).toBe("inventory-incomplete");
  });

  it("treats a listing that rejected rows as incomplete even when it declared completeness", () => {
    // The parser downgrades a declared-complete payload that lost rows; the restore must honor
    // that downgrade rather than believing the original claim.
    const parsed = parseSessionsPayload("machine", {
      completeness: "complete",
      sessions: [
        { sessionId: "exact", workspaceId: "workspace", worktree: null, running: false, daemonEpoch: "epoch" },
        { sessionId: 7, workspaceId: "workspace" },
      ],
    });
    if (!parsed.ok) throw new Error("Invalid test inventory");
    expect(parsed.inventory.rejectedRowCount).toBeGreaterThan(0);
    expect(parsed.inventory.completeness).not.toBe("complete");
    expect(resolveAccountContinuity(pointer, { ...discovery(), sessionInventories: { machine: parsed.inventory } }).kind)
      .toBe("inventory-incomplete");
  });

  it.each(["partial", "unknown"] as const)("still resolves a live exact session from a %s listing", (completeness) => {
    const state = discovery();
    state.sessionInventories.machine.completeness = completeness;
    const result = resolveAccountContinuity(pointer, state);
    expect(result.kind).toBe("session-alive");
    if (result.kind === "session-alive") expect(result.option.sessionId).toBe("exact");
  });

  it("treats a malformed or never-listed session listing as retryable, not as a dead session", () => {
    // A payload that failed to parse leaves no inventory at all while the machine status still
    // reads ready; that is a transport answer about the listing, so the restore stays retryable.
    const state = discovery();
    delete (state.sessionInventories as Record<string, unknown>).machine;
    const result = resolveAccountContinuity(pointer, state);
    expect(result.kind).toBe("network-error");
  });

  it("distinguishes absence, loading, removed and offline machines, missing worktrees and network errors", () => {
    expect(resolveAccountContinuity(null, discovery()).kind).toBe("absent");
    expect(resolveAccountContinuity({ ...pointer, machineId: null, workspaceId: null, sessionId: null }, discovery()).kind).toBe("absent");
    expect(resolveAccountContinuity(pointer, { ...discovery(), initialized: false }).kind).toBe("loading");
    expect(resolveAccountContinuity(pointer, { ...discovery(), machines: [] }).kind).toBe("machine-unknown");
    expect(resolveAccountContinuity(pointer, { ...discovery(), machines: [{ ...machine, online: false }] }).kind).toBe("machine-offline");
    expect(resolveAccountContinuity(pointer, { ...discovery(), accountOptions: [] }).kind).toBe("worktree-gone");
    expect(resolveAccountContinuity(pointer, { ...discovery(), error: "failed" }).kind).toBe("network-error");
    expect(resolveAccountContinuity(pointer, { ...discovery(), machineStatuses: {
      machine: { machine, status: "error", options: [], errorCode: "GRANT_MISSING" },
    } }).kind).toBe("grant-missing");
  });

  it.each(["SESSION_GONE", "EPOCH_CHANGED", "SSH_NO_TERMINAL"] as const)(
    "reports a refused restore as the session being gone only for the typed cause %s",
    (code) => {
      expect(continuityRestoreFailureFor({ code })).toBe("session-gone");
    },
  );

  it.each(["SERVER", "LIST_FAILED", "INVENTORY_INCOMPLETE", "EPOCH_UNKNOWN", "CREATE_FAILED", "SELECT_FAILED", "ANYTHING_ELSE"] as const)(
    "keeps a %s refusal retryable instead of claiming the session ended",
    (code) => {
      // A 5xx, a timed-out or unparsable listing, an unconfirmed attach and a rejected pick
      // are all answers about the attempt, not evidence the session stopped running.
      expect(continuityRestoreFailureFor({ code })).toBe("network-error");
    },
  );

  it("never claims a transport failure killed the session", () => {
    // A dropped fetch or abort reaches the catch with no typed code at all.
    expect(continuityRestoreFailureFor(null)).toBe("network-error");
  });
});
