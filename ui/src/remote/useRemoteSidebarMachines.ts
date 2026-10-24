// Which machines the remote sidebar reads, and how: account mode reads every machine of the
// account over its tunnel, a direct pairing reads its one gateway. Change frames arrive through
// the frame bus RemoteApp already feeds from its single events socket.
import { useMemo, useRef } from "react";

import type { AccountMachineView } from "./accountSession";
import { discoverySidebarModel } from "./accountSidebarFallback";
import type { SupervisorFrameBus } from "./home/machineSupervisorSources";
import { tunnelSupervisorFetch, type MachineLink } from "./home/machineSupervisorSources";
import type { RemoteSessionInventory } from "./remoteSessionInventory";
import type { RemoteSidebarSelection } from "./remoteSidebarLive";
import type { SidebarFetch } from "./remoteSidebarSource";
import type { MachineDiscoveryStatus } from "./useAccountWorktrees";
import type { RemoteSidebarMachine } from "./useRemoteSidebarModel";

/** The machine id frames of a direct (non-account) pairing are published under. */
export const DIRECT_SIDEBAR_MACHINE = "direct";

type AccountInput = {
  readonly kind: "account";
  readonly machines: readonly AccountMachineView[];
  readonly linkFor: (machineId: string) => MachineLink | null;
  readonly inventories: Readonly<Record<string, RemoteSessionInventory>>;
  /** Discovery per machine: its last listing, and whether its connect failed. */
  readonly statuses: Readonly<Record<string, MachineDiscoveryStatus>>;
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
      machines = input.machines.map((machine): RemoteSidebarMachine => {
        const status = input.statuses[machine.machineId];
        const offline = machine.online === false || status?.status === "offline";
        // An online machine is only read over an open tunnel; without one it keeps its last list.
        const link = offline ? null : input.linkFor(machine.machineId);
        return {
          machineId: machine.machineId,
          label: machine.displayName || machine.machineId,
          fetchLike: link ? tunnelSupervisorFetch(link.transport, link.deviceToken) : null,
          subscribe: subscribe(machine.machineId),
          connectionId: link ? `${idOf(link.transport)}:${link.deviceToken}` : "unlinked",
          inventory: input.inventories[machine.machineId] ?? null,
          selection: machine.machineId === input.activeMachineId ? selection : null,
          unreachable: offline || status?.status === "error",
          fallback: link ? null : discoverySidebarModel(status),
        };
      });
      break;
    default: {
      const unreachable: never = input;
      return unreachable;
    }
  }
  // The model hook keys subscriptions by id and connection; live state follows the inventories.
  const key = machines
    .map((m) => `${m.machineId}\u0000${m.connectionId ?? ""}\u0000${m.inventory?.revision ?? ""}\u0000${m.inventory?.fetchedAt ?? ""}\u0000${m.unreachable ? 1 : 0}`)
    .join("\u0001");
  const selectionKey = selection ? `${selection.workspaceId}\u0000${selection.worktreeSlug ?? ""}` : "";
  // key, selectionKey and the discovery statuses carry every input the list is built from.
  const statuses = input.kind === "account" ? input.statuses : null;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => machines, [key, selectionKey, statuses]);
}
