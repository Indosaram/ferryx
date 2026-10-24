// Joins account discovery, the active machine connection and the remote supervisor feed into
// what the phone home renders: one view per machine, merged lanes, pause, and resync.
import { useCallback, useMemo } from "react";

import type { AccountConnection } from "../accountSession";
import { setRemoteLanePaused, SUPERVISOR_OFFLINE, useRemoteSupervisor, type LanePauseResult, type RemoteSupervisorLane } from "../remoteSupervisor";
import type { UseAccountWorktreesResult } from "../useAccountWorktrees";
import { useMachineSupervisorSources, type MachineLink, type SupervisorFrameBus } from "./machineSupervisorSources";
import type { SupervisorMachineView } from "./phoneHomeModel";

export type PhoneSupervisorInput = {
  readonly enabled: boolean;
  readonly discovery: Pick<UseAccountWorktreesResult, "machines" | "machineStatuses" | "peekConnection">;
  readonly active: Pick<AccountConnection, "transport" | "deviceToken" | "machine" | "isClosed"> | null;
  readonly bus: SupervisorFrameBus;
};

export type PhoneSupervisor = {
  readonly machines: SupervisorMachineView[];
  readonly lanes: RemoteSupervisorLane[];
  readonly now: number;
  readonly setPaused: (machineId: string, laneKey: string, paused: boolean) => Promise<LanePauseResult>;
  readonly resync: (machineId?: string) => void;
};

export function usePhoneSupervisor({ enabled, discovery, active, bus }: PhoneSupervisorInput): PhoneSupervisor {
  const linkFor = (machineId: string): MachineLink | null => {
    // The active connection wins: discovery handed its tunnel over and no longer pools it.
    if (active && !active.isClosed && active.machine.machineId === machineId) {
      return { transport: active.transport, deviceToken: active.deviceToken };
    }
    return discovery.peekConnection(machineId);
  };
  const { sources, fetchFor, resync } = useMachineSupervisorSources({ enabled, machines: discovery.machines, linkFor, bus });
  const feed = useRemoteSupervisor({ machines: sources });

  const machines = useMemo(() => {
    if (!enabled) return [];
    return discovery.machines.map((machine): SupervisorMachineView => {
      const label = machine.displayName || machine.machineId;
      const entry = feed.machines.find((item) => item.machineId === machine.machineId);
      if (entry) return { machineId: machine.machineId, label, state: entry.state, snapshot: entry.snapshot, error: entry.error };
      const status = discovery.machineStatuses[machine.machineId]?.status;
      const state: SupervisorMachineView["state"] =
        machine.online === false || status === "offline"
          ? "offline"
          : status === "error"
            ? "error"
            : status === "ready"
              ? "not-connected"
              : "loading";
      const discoveryError = discovery.machineStatuses[machine.machineId];
      return {
        machineId: machine.machineId,
        label,
        state,
        snapshot: null,
        error: state === "error" ? { code: discoveryError?.errorCode ?? "UNAVAILABLE", message: discoveryError?.error ?? "" } : null,
      };
    });
  }, [enabled, discovery.machines, discovery.machineStatuses, feed.machines]);

  const setPaused = useCallback(
    async (machineId: string, laneKey: string, paused: boolean): Promise<LanePauseResult> => {
      const fetchLike = fetchFor(machineId);
      if (!fetchLike) {
        return { kind: "error", code: SUPERVISOR_OFFLINE, message: "This computer is not connected right now.", status: null };
      }
      const result = await setRemoteLanePaused(fetchLike, laneKey, paused);
      // A pooled discovery tunnel carries no change frames, so ask for the new snapshot directly.
      if (result.kind === "ok") resync(machineId);
      return result;
    },
    [fetchFor, resync],
  );

  return { machines, lanes: feed.lanes, now: feed.now, setPaused, resync };
}
