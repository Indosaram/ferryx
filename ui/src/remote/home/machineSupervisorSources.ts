// Adapters from the account's machine connections to useRemoteSupervisor inputs. Each machine
// answers over its relay tunnel (the active connection or a pooled discovery tunnel). Change frames come from the one events
// socket RemoteApp already owns, relayed per machine through a small frame bus.
import { useCallback, useMemo, useRef } from "react";

import type { AccountMachineView } from "../accountSession";
import type { TunnelTransport } from "../attachTunnel";
import type { RemoteSupervisorMachine, SupervisorFetch, SupervisorHttpResponse } from "../remoteSupervisor";

type FrameHandler = (frame: unknown) => void;

/** Fan-out of `/api/v1/events` frames keyed by machine id. */
export type SupervisorFrameBus = {
  readonly emit: (machineId: string, frame: unknown) => void;
  readonly subscribe: (machineId: string, handler: FrameHandler) => () => void;
};

export function createSupervisorFrameBus(): SupervisorFrameBus {
  const handlers = new Map<string, Set<FrameHandler>>();
  return {
    emit(machineId, frame) {
      for (const handler of handlers.get(machineId) ?? []) handler(frame);
    },
    subscribe(machineId, handler) {
      const set = handlers.get(machineId) ?? new Set<FrameHandler>();
      set.add(handler);
      handlers.set(machineId, set);
      return () => {
        set.delete(handler);
      };
    },
  };
}

/** The frame the hook treats as "you may have missed changes": it refetches once. */
export const SUPERVISOR_RESYNC_FRAME = { type: "inventoryInvalidated", reason: "subscribe" } as const;

function settleResponse(res: SupervisorHttpResponse | undefined | null): SupervisorHttpResponse {
  // A transport that resolves without a response is treated like one that threw: offline.
  if (!res || typeof res.status !== "number") throw new Error("Machine returned no response");
  return res;
}

/** Tunnel adapter: binds the device token so callers pass only the path. */
export function tunnelSupervisorFetch(transport: Pick<TunnelTransport, "fetchLike">, deviceToken: string): SupervisorFetch {
  return async (path, init) =>
    settleResponse(
      await transport.fetchLike(path, {
        method: init?.method,
        headers: { Authorization: `Bearer ${deviceToken}`, ...(init?.headers ?? {}) },
        body: init?.body,
      }),
    );
}

/** One machine's live way in, or null when no tunnel to it is open right now. */
export type MachineLink = { readonly transport: Pick<TunnelTransport, "fetchLike">; readonly deviceToken: string };

export type SupervisorSourceInput = {
  readonly enabled: boolean;
  readonly machines: readonly AccountMachineView[];
  readonly linkFor: (machineId: string) => MachineLink | null;
  readonly bus: SupervisorFrameBus;
};

export type UnlinkedMachine = { readonly machineId: string; readonly label: string; readonly reason: "offline" | "not-connected" };

export type SupervisorSources = {
  readonly sources: RemoteSupervisorMachine[];
  readonly unlinked: UnlinkedMachine[];
  readonly fetchFor: (machineId: string) => SupervisorFetch | null;
  /** Asks one machine's supervisor feed to refetch (after a pause, or when the home opens). */
  readonly resync: (machineId?: string) => void;
};

export function useMachineSupervisorSources(input: SupervisorSourceInput): SupervisorSources {
  const { enabled, machines, linkFor, bus } = input;
  // Stable ids per transport object, so a replaced tunnel resubscribes and refetches.
  const idsRef = useRef(new WeakMap<object, string>());
  const nextIdRef = useRef(0);
  const idOf = (key: object): string => {
    let id = idsRef.current.get(key);
    if (!id) {
      nextIdRef.current += 1;
      id = String(nextIdRef.current);
      idsRef.current.set(key, id);
    }
    return id;
  };

  const links = machines.map((machine) => ({ machine, link: machine.online === false ? null : linkFor(machine.machineId) }));
  const linkKey = links.map(({ machine, link }) => `${machine.machineId}:${link ? idOf(link.transport) : "-"}:${link?.deviceToken ?? ""}`).join("|");

  const linksRef = useRef(links);
  linksRef.current = links;

  const value = useMemo(() => {
    const sources: RemoteSupervisorMachine[] = [];
    const unlinked: UnlinkedMachine[] = [];
    const fetches = new Map<string, SupervisorFetch>();
    if (enabled) {
      for (const { machine, link } of linksRef.current) {
        const label = machine.displayName || machine.machineId;
        if (!link) {
          unlinked.push({ machineId: machine.machineId, label, reason: machine.online === false ? "offline" : "not-connected" });
          continue;
        }
        const fetchLike = tunnelSupervisorFetch(link.transport, link.deviceToken);
        fetches.set(machine.machineId, fetchLike);
        sources.push({
          machineId: machine.machineId,
          label,
          fetchLike,
          subscribe: (handler) => bus.subscribe(machine.machineId, handler),
          capabilities: null,
          connectionId: idOf(link.transport),
        });
      }
    }
    return { sources, unlinked, fetches };
    // linkKey carries every input identity the memo depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, linkKey, bus]);

  const resync = useCallback(
    (machineId?: string) => {
      for (const source of value.sources) {
        if (!machineId || source.machineId === machineId) bus.emit(source.machineId, SUPERVISOR_RESYNC_FRAME);
      }
    },
    [bus, value.sources],
  );
  const fetchFor = useCallback((machineId: string) => value.fetches.get(machineId) ?? null, [value.fetches]);

  return { sources: value.sources, unlinked: value.unlinked, fetchFor, resync };
}
