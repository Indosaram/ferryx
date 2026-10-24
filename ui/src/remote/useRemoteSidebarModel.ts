// The remote's sidebar per machine, read from that machine's desktop. Same contract as the
// supervisor feed: subscribe first, then fetch; refetch only on change events or when the
// connection is replaced, never on a timer.
import { useEffect, useMemo, useRef, useState } from "react";

import type { SidebarLive, SidebarModel } from "../components/sidebar/sidebarModel";
import { remoteSidebarLive, type RemoteSidebarSelection } from "./remoteSidebarLive";
import { loadRemoteSidebar, SIDEBAR_OFFLINE, type RemoteSidebarResult, type SidebarFetch } from "./remoteSidebarSource";
import type { RemoteSessionInventory } from "./remoteSessionInventory";

export const SIDEBAR_CHANGED_FRAME = "sidebarChanged";

/**
 * One machine: the direct gateway, an account machine's tunnel, or a paired host behind the
 * relay's /host/{machineId} base. `fetchLike` carries the device token; `subscribe` delivers
 * every /api/v1/events frame. Change `connectionId` when the transport is replaced (a reconnect).
 */
export type RemoteSidebarMachine = {
  readonly machineId: string;
  readonly label: string;
  readonly fetchLike: SidebarFetch;
  readonly subscribe: (handler: (frame: unknown) => void) => () => void;
  readonly connectionId?: string;
  readonly inventory: RemoteSessionInventory | null;
  readonly selection: RemoteSidebarSelection | null;
};

export type RemoteSidebarState = "loading" | "ok" | "offline" | "error";

export type RemoteSidebarSection = {
  readonly machineId: string;
  /** The machine name the section is headed with when an account has several machines. */
  readonly label: string;
  readonly state: RemoteSidebarState;
  /** The last model that loaded; kept while a refetch fails so the section does not blank. */
  readonly model: SidebarModel | null;
  readonly live: SidebarLive | null;
  readonly error: { code: string; message: string } | null;
};

type Settled = Pick<RemoteSidebarSection, "state" | "model" | "error">;

function frameOf(frame: unknown): Record<string, unknown> | null {
  let value = frame;
  if (typeof frame === "string") {
    try {
      value = JSON.parse(frame);
    } catch (err) {
      if (err instanceof SyntaxError) return null;
      throw err;
    }
  }
  return value !== null && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

/**
 * The published model changes only with `sidebarChanged`; a fresh or lagged socket may have
 * missed one. A model rebuilt from the inventory also goes stale on any inventory change.
 */
function wantsRefetch(frame: unknown, source: SidebarModel["source"] | null): boolean {
  const event = frameOf(frame);
  if (event?.type === SIDEBAR_CHANGED_FRAME) return true;
  if (event?.type !== "inventoryInvalidated") return false;
  return event.reason === "subscribe" || event.reason === "lag" || source === "inventory";
}

function settle(previous: Settled | undefined, result: RemoteSidebarResult): Settled {
  switch (result.kind) {
    case "ok":
      return { state: "ok", model: result.model, error: null };
    case "error":
      return { state: result.code === SIDEBAR_OFFLINE ? "offline" : "error", model: previous?.model ?? null, error: result };
    default: {
      const unreachable: never = result;
      return unreachable;
    }
  }
}

export function useRemoteSidebarModel({ machines }: { readonly machines: readonly RemoteSidebarMachine[] }): RemoteSidebarSection[] {
  const machinesRef = useRef(machines);
  machinesRef.current = machines;
  const [settled, setSettled] = useState<Record<string, Settled>>({});
  const settledRef = useRef(settled);
  settledRef.current = settled;
  const key = machines.map((m) => `${m.machineId}\u0000${m.connectionId ?? ""}`).join("\u0001");

  useEffect(() => {
    const cleanups: Array<() => void> = [];
    for (const machine of machinesRef.current) {
      const { machineId } = machine;
      let alive = true;
      let inFlight = false;
      let again = false;
      // A change during a fetch queues exactly one follow-up fetch instead of racing a second.
      const refetch = async () => {
        if (inFlight) {
          again = true;
          return;
        }
        inFlight = true;
        do {
          again = false;
          const current = machinesRef.current.find((m) => m.machineId === machineId) ?? machine;
          const result = await loadRemoteSidebar(current.fetchLike);
          if (!alive) return;
          setSettled((prev) => ({ ...prev, [machineId]: settle(prev[machineId], result) }));
        } while (again);
        inFlight = false;
      };
      const unsubscribe = machine.subscribe((frame) => {
        if (wantsRefetch(frame, settledRef.current[machineId]?.model?.source ?? null)) void refetch();
      });
      void refetch();
      cleanups.push(() => {
        alive = false;
        unsubscribe();
      });
    }
    return () => {
      for (const cleanup of cleanups) cleanup();
    };
  }, [key]);

  return useMemo(
    () =>
      machines.map((m): RemoteSidebarSection => {
        const entry = settled[m.machineId] ?? { state: "loading", model: null, error: null };
        return {
          machineId: m.machineId,
          label: m.label,
          ...entry,
          live: entry.model ? remoteSidebarLive(entry.model, m.inventory, m.selection) : null,
        };
      }),
    [machines, settled],
  );
}
