// Joins account discovery, the active machine connection and the remote supervisor feed into
// what the phone home renders: one view per machine, merged lanes, pause, and resync, plus the
// one account supervisor: the machine holding the supervisor lease. Every other machine is on
// standby; the phone never starts a second supervisor, it can only move the one there is.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { isStandbyRole } from "../../lib/supervisorModel";
import type { SupervisorConnection, SupervisorMoveResult, UnifiedSupervisorClient, UnifiedSupervisorView } from "../../lib/unifiedSupervisor";
import type { AccountConnection } from "../accountSession";
import {
  adoptRemoteArchivedGoals,
  moveRemoteSupervisor,
  remoteUnifiedClient,
  setRemoteLanePaused,
  standbyHolderOf,
  SUPERVISOR_OFFLINE,
  useRemoteSupervisor,
  type AdoptArchivedResult,
  type LanePauseResult,
  type RemoteSupervisorLane,
  type SupervisorFetch,
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
  /** Moves the account supervisor to this machine (`force` after the owner confirmed the loss window). */
  readonly moveHere: (machineId: string, force: boolean) => Promise<SupervisorMoveResult>;
  /** Adopts open goals of a standby machine's archived supervisor into the account supervisor. */
  readonly adoptArchived: (machineId: string, goalIds: readonly string[]) => Promise<AdoptArchivedResult>;
  readonly resync: (machineId?: string) => void;
  /** The one account supervisor host, live or last seen; null before any machine named one. */
  readonly host: UnifiedHostView | null;
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
 * What the account host choice remembers across snapshots: the last view per machine and the
 * last machine known to hold the lease. Updated in place.
 */
export type HostMemory = {
  readonly lastSeen: Map<string, UnifiedSupervisorView>;
  lastHolder: string | null;
};

export function newHostMemory(): HostMemory {
  return { lastSeen: new Map(), lastHolder: null };
}

function holdsLease(machine: SupervisorMachineView): boolean {
  const role = machine.snapshot?.hub.role;
  return role === "holder" || role === "provisional";
}

/**
 * The holder hint a SUPERVISOR_STANDBY refusal or a move left, while it is still needed: once
 * any snapshot reports a holder (or provisional) role the snapshots decide, so the hint is dropped.
 */
export function pendingHint(machines: readonly SupervisorMachineView[], hint: string | null): string | null {
  return machines.some(holdsLease) ? null : hint;
}

/**
 * The one account supervisor host among the machine views. The machine whose snapshot reports
 * the lease role `holder` wins; otherwise the holder a standby machine names, then the holder a
 * SUPERVISOR_STANDBY refusal or a move named (`hint`); machines that report no lease role (older
 * daemons) count as holders only while no machine reports one; last, the last known holder, shown
 * with its last view and never replaced by another machine's supervisor.
 */
export function accountHost(
  machines: readonly SupervisorMachineView[],
  memory: HostMemory,
  clientFor: (machineId: string) => UnifiedSupervisorClient | null,
  hint: string | null = null,
): UnifiedHostView | null {
  for (const machine of machines) {
    const current = machine.snapshot?.supervisor ?? null;
    // A standby's own (archived) supervisor is never the account's.
    if (current?.identity && !isStandbyRole(machine.snapshot?.hub.role)) memory.lastSeen.set(machine.machineId, current);
  }
  const leaseAware = machines.some((machine) => machine.snapshot?.hub.role != null);
  const reported = machines.find(holdsLease);
  const legacy = leaseAware ? undefined : machines.find((machine) => machine.snapshot?.supervisor?.identity);
  const named = machines.find((machine) => isStandbyRole(machine.snapshot?.hub.role) && machine.snapshot?.hub.holderMachineId)?.snapshot?.hub;
  const holderId = reported?.machineId ?? named?.holderMachineId ?? hint ?? legacy?.machineId ?? memory.lastHolder;
  if (!holderId) return null;
  memory.lastHolder = holderId;
  const machine = machines.find((item) => item.machineId === holderId) ?? null;
  const view = memory.lastSeen.get(holderId);
  if (!view) return null;
  const current = machine && !isStandbyRole(machine.snapshot?.hub.role) ? (machine.snapshot?.supervisor ?? null) : null;
  const connection: SupervisorConnection =
    machine?.state === "ok" && current?.identity
      ? "live"
      : machine?.state === "ok" || machine?.state === "loading"
        ? "unavailable"
        : "disconnected";
  const error =
    connection === "live"
      ? null
      : (machine?.error ?? (machine?.state === "ok" ? { code: "SUPERVISOR_UNAVAILABLE", message: "" } : { code: SUPERVISOR_OFFLINE, message: "" }));
  return {
    machineId: holderId,
    label: machine?.label ?? named?.holderLabel ?? holderId,
    connection,
    error,
    view,
    client: connection === "live" ? clientFor(holderId) : null,
  };
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
  // The holder a SUPERVISOR_STANDBY refusal named: requests follow it until a snapshot agrees.
  const [standbyHint, setStandbyHint] = useState<string | null>(null);

  // Every supervisor call goes through this: a standby's 409 names the holder, so the home
  // switches to it and asks every machine for a fresh snapshot.
  const watchStandby = useCallback(
    (fetchLike: SupervisorFetch): SupervisorFetch =>
      async (path, init) => {
        const res = init ? await fetchLike(path, init) : await fetchLike(path);
        const holder = standbyHolderOf(res);
        if (holder) {
          setStandbyHint(holder.holderMachineId);
          resync();
        }
        return res;
      },
    [resync],
  );

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

  // One client per machine link, so views keyed on the client do not refetch every render.
  const clients = useMemo(() => {
    const map = new Map<string, UnifiedSupervisorClient>();
    for (const machine of discovery.machines) {
      const fetchLike = fetchFor(machine.machineId);
      if (fetchLike) map.set(machine.machineId, remoteUnifiedClient(watchStandby(fetchLike), () => resync(machine.machineId)));
    }
    return map;
  }, [discovery.machines, fetchFor, resync, watchStandby]);
  const memory = useRef(newHostMemory());
  const hint = pendingHint(machines, standbyHint);
  // A snapshot reported the holder: forget the hint so a later move or failover is not overridden.
  useEffect(() => {
    if (standbyHint !== null && hint === null) setStandbyHint(null);
  }, [standbyHint, hint]);
  const host = useMemo(
    () => accountHost(machines, memory.current, (machineId) => clients.get(machineId) ?? null, hint),
    [machines, clients, hint],
  );
  const holderId = host?.machineId ?? null;

  const setPaused = useCallback(
    async (machineId: string, laneKey: string, paused: boolean): Promise<LanePauseResult> => {
      // Lanes are driven by the account supervisor: a standby's pause goes to the holder.
      const standby = isStandbyRole(machines.find((machine) => machine.machineId === machineId)?.snapshot?.hub.role);
      const target = standby && holderId ? holderId : machineId;
      const fetchLike = fetchFor(target);
      if (!fetchLike) {
        return { kind: "error", code: SUPERVISOR_OFFLINE, message: "This computer is not connected right now.", status: null };
      }
      const result = await setRemoteLanePaused(watchStandby(fetchLike), laneKey, paused);
      // A pooled discovery tunnel carries no change frames, so ask for the new snapshot directly.
      if (result.kind === "ok") resync(target);
      return result;
    },
    [fetchFor, holderId, machines, resync, watchStandby],
  );

  const moveHere = useCallback(
    async (machineId: string, force: boolean): Promise<SupervisorMoveResult> => {
      const fetchLike = fetchFor(machineId);
      if (!fetchLike) {
        return { kind: "error", code: SUPERVISOR_OFFLINE, message: "This computer is not connected right now.", status: null };
      }
      const result = await moveRemoteSupervisor(fetchLike, force);
      if (result.kind === "ok") {
        setStandbyHint(machineId);
        // Both the old and the new holder changed role.
        resync();
      }
      return result;
    },
    [fetchFor, resync],
  );

  const adoptArchived = useCallback(
    async (machineId: string, goalIds: readonly string[]): Promise<AdoptArchivedResult> => {
      const fetchLike = fetchFor(machineId);
      if (!fetchLike) {
        return { kind: "error", code: SUPERVISOR_OFFLINE, message: "This computer is not connected right now.", status: null };
      }
      const result = await adoptRemoteArchivedGoals(fetchLike, goalIds);
      if (result.kind === "ok") resync();
      return result;
    },
    [fetchFor, resync],
  );

  return { machines, lanes: feed.lanes, now: feed.now, setPaused, moveHere, adoptArchived, resync, host };
}
