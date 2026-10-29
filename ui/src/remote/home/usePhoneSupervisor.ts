// Joins account discovery, the active machine connection and the remote supervisor feed into
// what the phone home renders: one view per machine, merged lanes, pause, and resync.
import { useCallback, useMemo, useRef } from "react";

import type { SupervisorConnection, UnifiedSupervisorClient, UnifiedSupervisorView } from "../../lib/unifiedSupervisor";
import type { AccountConnection } from "../accountSession";
import {
  remoteUnifiedClient,
  setRemoteLanePaused,
  startRemoteSupervisor,
  SUPERVISOR_OFFLINE,
  useRemoteSupervisor,
  type HubStartResult,
  type LanePauseResult,
  type RemoteSupervisorLane,
} from "../remoteSupervisor";
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
  readonly startHub: (machineId: string) => Promise<HubStartResult>;
  readonly resync: (machineId?: string) => void;
  /** Every machine that hosts (or last hosted) the unified supervisor. */
  readonly unified: UnifiedHostView[];
};

/** One supervisor host as the phone reaches it. `view` is the last one seen while it is not live. */
export type UnifiedHostView = {
  readonly machineId: string;
  readonly label: string;
  readonly connection: SupervisorConnection;
  readonly error: { code: string; message: string } | null;
  readonly view: UnifiedSupervisorView;
  /** Null while the host is not live: nothing can be sent. */
  readonly client: UnifiedSupervisorClient | null;
};

/**
 * The unified supervisor hosts among the machine views: a machine whose snapshot names a
 * supervisor identity, or one that did before and is now unreachable (shown with its last view,
 * never replaced by another machine's supervisor). `lastSeen` is updated in place.
 */
export function unifiedHosts(
  machines: readonly SupervisorMachineView[],
  lastSeen: Map<string, UnifiedSupervisorView>,
  clientFor: (machineId: string) => UnifiedSupervisorClient | null,
): UnifiedHostView[] {
  const hosts: UnifiedHostView[] = [];
  for (const machine of machines) {
    const current = machine.snapshot?.supervisor ?? null;
    if (current?.identity) lastSeen.set(machine.machineId, current);
    const view = current?.identity ? current : lastSeen.get(machine.machineId);
    if (!view) continue;
    const connection: SupervisorConnection =
      machine.state === "ok" && current?.identity
        ? "live"
        : machine.state === "ok" || machine.state === "loading"
          ? "unavailable"
          : "disconnected";
    const error =
      connection === "live"
        ? null
        : (machine.error ?? (machine.state === "ok" ? { code: "SUPERVISOR_UNAVAILABLE", message: "" } : { code: SUPERVISOR_OFFLINE, message: "" }));
    hosts.push({
      machineId: machine.machineId,
      label: machine.label,
      connection,
      error,
      view,
      client: connection === "live" ? clientFor(machine.machineId) : null,
    });
  }
  return hosts;
}

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

  const startHub = useCallback(
    async (machineId: string): Promise<HubStartResult> => {
      const fetchLike = fetchFor(machineId);
      if (!fetchLike) {
        return { kind: "error", code: SUPERVISOR_OFFLINE, message: "This computer is not connected right now.", status: null };
      }
      const result = await startRemoteSupervisor(fetchLike);
      if (result.kind === "ok") resync(machineId);
      return result;
    },
    [fetchFor, resync],
  );

  // One client per machine link, so views keyed on the client do not refetch every render.
  const clients = useMemo(() => {
    const map = new Map<string, UnifiedSupervisorClient>();
    for (const machine of discovery.machines) {
      const fetchLike = fetchFor(machine.machineId);
      if (fetchLike) map.set(machine.machineId, remoteUnifiedClient(fetchLike, () => resync(machine.machineId)));
    }
    return map;
  }, [discovery.machines, fetchFor, resync]);
  const lastSeen = useRef(new Map<string, UnifiedSupervisorView>());
  const unified = useMemo(
    () => unifiedHosts(machines, lastSeen.current, (machineId) => clients.get(machineId) ?? null),
    [machines, clients],
  );

  return { machines, lanes: feed.lanes, now: feed.now, setPaused, startHub, resync, unified };
}
