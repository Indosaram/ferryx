// The remote's sidebar per machine, read from that machine's desktop. Same contract as the
// supervisor feed: subscribe first, then fetch; refetch only on change events or when the
// connection is replaced, never on a timer.
import { useEffect, useMemo, useRef, useState } from "react";

import type { SidebarLiveSignals, SidebarModel } from "../components/sidebar/sidebarModel";
import type { AttentionBySessionId } from "../lib/attentionGroup";
import { remoteSidebarSignals, type RemoteSidebarSelection, type RemoteSidebarSupervisor } from "./remoteSidebarLive";
import { loadRemoteSidebar, SIDEBAR_OFFLINE, type RemoteSidebarResult, type SidebarFetch } from "./remoteSidebarSource";
import type { RemoteSessionInventory } from "./remoteSessionInventory";
import type { LaneLifecycleModel } from "../lib/sidebarPersistence";
import { isResyncBoundaryFrame } from "./remoteSupervisor";
import { applyLaneLifecycleModel, parsePanelFrame } from "./supervisorPanel/panelModel";

export const SIDEBAR_CHANGED_FRAME = "sidebarChanged";

/**
 * One machine: the direct gateway, an account machine's tunnel, or a paired host behind the
 * relay's /host/{machineId} base. `fetchLike` carries the device token; `subscribe` delivers
 * every /api/v1/events frame. Change `connectionId` when the transport is replaced (a reconnect).
 */
export type RemoteSidebarMachine = {
  readonly machineId: string;
  readonly label: string;
  /** Null while no connection to the machine is open: nothing is fetched or subscribed. */
  readonly fetchLike: SidebarFetch | null;
  readonly subscribe: (handler: (frame: unknown) => void) => () => void;
  readonly connectionId?: string;
  readonly inventory: RemoteSessionInventory | null;
  readonly selection: RemoteSidebarSelection | null;
  /** Presence says offline, or the last connect failed: the only cases worded as unreachable. */
  readonly unreachable?: boolean;
  /** The list shown while unconnected before any model loaded (built from discovery). */
  readonly fallback?: SidebarModel | null;
};

/** `disconnected`: online but no connection is open; the last or fallback list stays, unworded. */
export type RemoteSidebarState = "loading" | "ok" | "disconnected" | "offline" | "error";

export type RemoteSidebarSection = {
  readonly machineId: string;
  /** The machine name the section is headed with when an account has several machines. */
  readonly label: string;
  readonly state: RemoteSidebarState;
  /** The last model that loaded; kept while a refetch fails so the section does not blank. */
  readonly model: SidebarModel | null;
  /** Session signals for sidebarLiveState; the view applies its filter when deriving live state. */
  readonly signals: SidebarLiveSignals | null;
  readonly error: { code: string; message: string } | null;
};

type Settled = Pick<RemoteSidebarSection, "state" | "model" | "error">;

const UNREACHABLE_ERROR = { code: SIDEBAR_OFFLINE, message: "This computer is not reachable" } as const;

/** A machine with no open connection: never fetched, so its state comes from presence alone. */
function unconnected(machine: RemoteSidebarMachine, previous: Settled | undefined): Settled {
  const model = previous?.model ?? machine.fallback ?? null;
  if (machine.unreachable) return { state: "offline", model, error: UNREACHABLE_ERROR };
  return model ? { state: "disconnected", model, error: null } : { state: "loading", model: null, error: null };
}

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
 * The published model changes only with `sidebarChanged`; a fresh, lagged or restarted socket may
 * have missed one, which the shared boundary decoder names. A model rebuilt from the inventory
 * also goes stale on any inventory change.
 */
function wantsRefetch(frame: unknown, source: SidebarModel["source"] | null): boolean {
  const event = frameOf(frame);
  if (event?.type === SIDEBAR_CHANGED_FRAME) return true;
  if (event?.type !== "inventoryInvalidated") return false;
  return isResyncBoundaryFrame(event) || source === "inventory";
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

export type RemoteSidebarModelInput = {
  readonly machines: readonly RemoteSidebarMachine[];
  /** Each machine's hub supervisor, by machine id; drives the "supervisor typing" row slot. */
  readonly supervisors?: Readonly<Record<string, RemoteSidebarSupervisor>>;
  /** Each machine's daemon attention groups by backend session id (its supervisor panel snapshot). */
  readonly attention?: Readonly<Record<string, AttentionBySessionId>>;
};

export function useRemoteSidebarModel({ machines, supervisors, attention }: RemoteSidebarModelInput): RemoteSidebarSection[] {
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
      if (!machine.fetchLike) continue;
      let alive = true;
      let inFlight = false;
      let again = false;
      let lifecycle: LaneLifecycleModel | null = null;
      // A change during a fetch queues exactly one follow-up fetch instead of racing a second.
      const refetch = async () => {
        if (inFlight) {
          again = true;
          return;
        }
        inFlight = true;
        try {
          do {
            again = false;
            const current = machinesRef.current.find((m) => m.machineId === machineId) ?? machine;
            if (!current.fetchLike) return;
            let result: RemoteSidebarResult;
            try {
              result = await loadRemoteSidebar(current.fetchLike);
            } catch (err) {
              // A bug in the builder must not wedge this machine: the next change event retries.
              result = { kind: "error", code: "LOAD_FAILED", message: err instanceof Error ? err.message : String(err) };
            }
            if (!alive) return;
            const racedLifecycle = lifecycle;
            setSettled((prev) => {
              const next = settle(prev[machineId], result);
              return { ...prev, [machineId]: next.model && racedLifecycle
                ? { ...next, model: applyLaneLifecycleModel(next.model, racedLifecycle) }
                : next };
            });
          } while (again);
        } finally {
          inFlight = false;
        }
      };
      const unsubscribe = machine.subscribe((frame) => {
        const event = parsePanelFrame(frame);
        if (event?.type === "panel.lanes") {
          const rows = new Map(lifecycle?.sections.flatMap((section) => section.rows.map((row) => [row.worktreeKey, row] as const)));
          for (const section of event.model.sections) {
            for (const row of section.rows) {
              if ((row.revision ?? -Infinity) >= (rows.get(row.worktreeKey)?.revision ?? -Infinity)) rows.set(row.worktreeKey, row);
            }
          }
          const sections = new Map<string, LaneLifecycleModel["sections"][number]>();
          for (const row of rows.values()) {
            if (!sections.has(row.workspaceId)) sections.set(row.workspaceId, { workspaceId: row.workspaceId, rows: [] });
            sections.get(row.workspaceId)!.rows.push(row);
          }
          lifecycle = { sections: [...sections.values()], revision: event.model.revision };
          setSettled((previous) => {
            const current = previous[machineId];
            return current?.model
              ? { ...previous, [machineId]: { ...current, model: applyLaneLifecycleModel(current.model, event.model) } }
              : previous;
          });
          return;
        }
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
        const entry = m.fetchLike
          ? settled[m.machineId] ?? { state: "loading", model: null, error: null }
          : unconnected(m, settled[m.machineId]);
        return {
          machineId: m.machineId,
          label: m.label,
          ...entry,
          signals: entry.model ? remoteSidebarSignals(entry.model, m.inventory, m.selection, supervisors?.[m.machineId] ?? null, attention?.[m.machineId]) : null,
        };
      }),
    [machines, settled, supervisors, attention],
  );
}
