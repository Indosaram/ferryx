import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import {
  canRestartPreviousTerminals,
  daemonUpdatePendingFrom,
  pendingUpdateMessage,
  rereadDaemonUpdatePending,
  restartConfirmMessage,
  restartPreviousTerminals,
} from "./updaterDaemonPending";

describe("daemon update pending state", () => {
  beforeEach(() => invoke.mockReset());

  it("reads the typed error and enables restart only for a known count", () => {
    const pending = daemonUpdatePendingFrom({
      code: "DAEMON_UPDATE_PENDING",
      details: { runningVersion: "2026.1013.2", liveSessions: 3, retireRequested: true },
    });
    expect(pending).toEqual({ runningVersion: "2026.1013.2", liveSessions: 3 });
    expect(canRestartPreviousTerminals(pending!)).toBe(true);
    expect(pendingUpdateMessage(pending!)).toBe(
      "Update installed; 3 terminals still running on the previous version",
    );
    expect(restartConfirmMessage(3)).toContain("3 running terminals");
    expect(daemonUpdatePendingFrom({ code: "DAEMON_PROTOCOL_MISMATCH", details: {} })).toBeNull();
  });

  it("an unknown count disables restart and never reaches the backend", async () => {
    const pending = daemonUpdatePendingFrom({
      code: "DAEMON_UPDATE_PENDING",
      details: { runningVersion: null, liveSessions: null },
    })!;
    expect(canRestartPreviousTerminals(pending)).toBe(false);
    expect(pendingUpdateMessage(pending)).toContain("cannot be managed from this version");
    expect(await restartPreviousTerminals(pending)).toEqual({ outcome: "unavailable" });
    expect(invoke).not.toHaveBeenCalled();
  });

  it("a malformed count is unknown, never zero", () => {
    for (const liveSessions of [undefined, "2", -1, 1.5]) {
      const pending = daemonUpdatePendingFrom({ code: "DAEMON_UPDATE_PENDING", details: { liveSessions } })!;
      expect(pending.liveSessions).toBeNull();
    }
  });

  it("confirm sends Shutdown once with the confirmed count; count changed re-reads", async () => {
    invoke.mockResolvedValueOnce({ outcome: "countChanged", live: 4 });
    const outcome = await restartPreviousTerminals({ runningVersion: "v", liveSessions: 3 });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("cmd_daemon_restart_previous_terminals", { confirmLiveSessions: 3 });
    expect(outcome).toEqual({ outcome: "countChanged", live: 4 });
    invoke.mockResolvedValueOnce({ runningVersion: "v", liveSessions: 4, retireRequested: true });
    expect(await rereadDaemonUpdatePending()).toEqual({ runningVersion: "v", liveSessions: 4 });
    expect(invoke).toHaveBeenLastCalledWith("cmd_daemon_update_pending_status");
  });
});
