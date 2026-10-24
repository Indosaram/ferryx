// Which machines the remote sidebar reads, and how: account mode reads every machine of the
// account over its tunnel, a direct pairing reads its one gateway. Change frames arrive through
// the frame bus RemoteApp already feeds from its single events socket.
import { useMemo, useRef } from "react";

import type { AccountMachineView } from "./accountSession";
import type { SupervisorFrameBus } from "./home/machineSupervisorSources";
import { tunnelSupervisorFetch, type MachineLink } from "./home/machineSupervisorSources";
import type { RemoteSessionInventory } from "./remoteSessionInventory";
import type { RemoteSidebarSelection } from "./remoteSidebarLive";
import type { SidebarFetch } from "./remoteSidebarSource";
import type { RemoteSidebarMachine } from "./useRemoteSidebarModel";

/** The machine id frames of a direct (non-account) pairing are published under. */
export const DIRECT_SIDEBAR_MACHINE = "direct";

/** An account machine without an open tunnel answers as offline, so its last list stays shown;
 * the next selection opens a tunnel, which changes the connection and refetches. */
const unlinkedFetch: SidebarFetch = () => Promise.reject(new Error("No open connection to this computer"));

type AccountInput = {
  readonly kind: "account";
  readonly machines: readonly AccountMachineView[];
  readonly linkFor: (machineId: string) => MachineLink | null;
  readonly inventories: Readonly<Record<string, RemoteSessionInventory>>;
  readonly activeMachineId: string | null;
};

type DirectInput = {
  readonly kind: "direct";
  /** Null until paired: no machine is read. */
  readonly fetchLike: SidebarFetch | null;
  readonly connectionId: string;
  /** The gateway's declared worktree and pane activity, shaped as a session inventory. */
  readonly inventory: RemoteSessionInventory | null;
};

export type RemoteSidebarMachinesInput = (AccountInput | DirectInput) & {
  readonly bus: SupervisorFrameBus;
  readonly selection: RemoteSidebarSelection | null;
};

export function useRemoteSidebarMachines(input: RemoteSidebarMachinesInput): RemoteSidebarMachine[] {
  // Stable ids per transport object, so a replaced tunnel resubscribes and refetches.
  const idsRef = useRef(new WeakMap<object, string>());
  const nextIdRef = useRef(0);
  const idOf = (key: object): string => {
    const known = idsRef.current.get(key);
    if (known) return known;
    nextIdRef.current += 1;
    const id = String(nextIdRef.current);
    idsRef.current.set(key, id);
    return id;
  };
  const { bus, selection } = input;
  const subscribe = (machineId: string) => (handler: (frame: unknown) => void) => bus.subscribe(machineId, handler);

  let machines: RemoteSidebarMachine[];
  switch (input.kind) {
    case "direct":
      machines = input.fetchLike
        ? [{
            machineId: DIRECT_SIDEBAR_MACHINE,
            label: "This computer",
            fetchLike: input.fetchLike,
            subscribe: subscribe(DIRECT_SIDEBAR_MACHINE),
            connectionId: input.connectionId,
            inventory: input.inventory,
            selection,
          }]
        : [];
      break;
    case "account":
      machines = input.machines.flatMap((machine): RemoteSidebarMachine[] => {
        if (machine.online === false) return [];
        const link = input.linkFor(machine.machineId);
        return [{
          machineId: machine.machineId,
          label: machine.displayName || machine.machineId,
          fetchLike: link ? tunnelSupervisorFetch(link.transport, link.deviceToken) : unlinkedFetch,
          subscribe: subscribe(machine.machineId),
          connectionId: link ? `${idOf(link.transport)}:${link.deviceToken}` : "unlinked",
          inventory: input.inventories[machine.machineId] ?? null,
          selection: machine.machineId === input.activeMachineId ? selection : null,
        }];
      });
      break;
    default: {
      const unreachable: never = input;
      return unreachable;
    }
  }
  // The model hook keys subscriptions by id and connection; live state follows the inventories.
  const key = machines
    .map((m) => `${m.machineId}\u0000${m.connectionId ?? ""}\u0000${m.inventory?.revision ?? ""}\u0000${m.inventory?.fetchedAt ?? ""}`)
    .join("\u0001");
  const selectionKey = selection ? `${selection.workspaceId}\u0000${selection.worktreeSlug ?? ""}` : "";
  // key and selectionKey carry every input the list is built from.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => machines, [key, selectionKey]);
}
