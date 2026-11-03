// The desktop's view of the hub supervisor: one snapshot from the daemon, refetched when the
// daemon emits `supervisor_changed`. Never polls; a short timer only expires the "acting" glow.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState, useSyncExternalStore } from "react";
import { EMPTY_SNAPSHOT, nextSupervisionExpiry, normalizeSnapshot, type SupervisorSnapshot } from "./supervisorModel";
import { moveRefusal, type SupervisorCallResult, type SupervisorMoveResult, type UnifiedSupervisorClient, type UnifiedSupervisorView } from "./unifiedSupervisor";

export {
  actingSessionIds,
  EMPTY_SNAPSHOT,
  nextSupervisionExpiry,
  normalizeSnapshot,
  paneSupervision,
  proposalAwaitingApproval,
  SUPERVISOR_ACTING_MS,
  SUPERVISOR_AWAITING_APPROVAL_LABEL,
  SUPERVISOR_HELD_MS,
  SUPERVISOR_STOP_REASON_LABELS,
  SUPERVISOR_STOP_REASONS,
  SUPERVISOR_STOPPED_FALLBACK_LABEL,
  supervisorHandoffs,
  supervisorStopReasonLabel,
  type PaneSupervision,
  type SupervisorAction,
  type SupervisorGoalMode,
  type SupervisorGoalView,
  type SupervisorHandoff,
  type SupervisorLane,
  type SupervisorRequirementStatus,
  type SupervisorSnapshot,
  type SupervisorStopReason,
} from "./supervisorModel";
import type { SupervisorGoalMode } from "./supervisorModel";

export const SUPERVISOR_CHANGED_EVENT = "supervisor_changed";
let snapshot: SupervisorSnapshot = EMPTY_SNAPSHOT;
/** The last unified supervisor view the daemon reported, kept while it is unreachable. */
let lastUnified: UnifiedSupervisorView | null = null;
let supported = true;
const listeners = new Set<() => void>();
let started: Promise<void> | null = null;

function publish(next: SupervisorSnapshot) {
  snapshot = next;
  if (next.supervisor) lastUnified = next.supervisor;
  for (const listener of listeners) listener();
}

export async function refreshSupervisor(): Promise<void> {
  try {
    publish(normalizeSnapshot(await invoke<unknown>("cmd_supervisor_snapshot")));
    supported = true;
  } catch (error) {
    // Show the off state rather than stale data, and say why: an older daemon reports that it
    // needs an upgrade, which is not the same as no hub.
    supported = false;
    const message = (error as { message?: unknown })?.message;
    publish({ ...EMPTY_SNAPSHOT, hub: { available: false, running: false, error: typeof message === "string" ? message : String(error) } });
  }
}

/** Subscribe before the first fetch so a change racing it is not lost. */
export function startSupervisorFeed(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  if (started) return started;
  started = (async () => {
    try {
      await listen(SUPERVISOR_CHANGED_EVENT, () => void refreshSupervisor());
    } catch {
      supported = false;
    }
    await refreshSupervisor();
  })();
  return started;
}

export async function setSupervisorLanePaused(laneKey: string, paused: boolean): Promise<void> {
  await invoke<boolean>("cmd_supervisor_set_paused", { laneKey, paused });
  await refreshSupervisor();
}

export type SupervisorGoalRequest =
  | { action: "set"; laneKey: string; requirements: unknown[]; mode?: SupervisorGoalMode; turnLimit?: number }
  | { action: "mode"; laneKey: string; mode: SupervisorGoalMode }
  | { action: "approve"; laneKey: string }
  | { action: "discard"; laneKey: string }
  | { action: "acceptRevision"; id: number }
  | { action: "rejectRevision"; id: number };

/** Steer a lane goal through the daemon (cmd_supervisor_goal), then refetch the snapshot. */
export async function supervisorGoal(request: SupervisorGoalRequest): Promise<void> {
  await invoke<unknown>("cmd_supervisor_goal", { request });
  await refreshSupervisor();
}

export function supervisorFeedSupported(): boolean {
  return supported;
}

/** A thrown IPC error as `{code, message, status, details}`; the UI branches on `code` only. */
function ipcRefusal(error: unknown): { code: string; message: string; status: number | null; details: unknown } {
  const raw = (error && typeof error === "object" ? error : {}) as { code?: unknown; message?: unknown; details?: unknown };
  const details = raw.details && typeof raw.details === "object" ? (raw.details as { status?: unknown }) : null;
  return {
    code: typeof raw.code === "string" && raw.code ? raw.code : "INTERNAL_ERROR",
    message: typeof raw.message === "string" ? raw.message : String(error),
    status: typeof details?.status === "number" ? details.status : null,
    details: raw.details ?? null,
  };
}

/**
 * Moves the account supervisor to this computer through its daemon (`cmd_supervisor_move_here`,
 * the same path as `ferryx hub move-here [--force]`), then refetches the snapshot.
 */
export async function moveSupervisorHere(force: boolean): Promise<SupervisorMoveResult> {
  try {
    const value = await invoke<unknown>("cmd_supervisor_move_here", { force });
    await refreshSupervisor();
    const moved = value && typeof value === "object" ? (value as { moved?: unknown }).moved : undefined;
    return { kind: "ok", moved: moved !== false };
  } catch (error) {
    const { code, message, status, details } = ipcRefusal(error);
    return moveRefusal(code, message, status, details);
  }
}

/** Adopts open goals of this computer's archived supervisor into the account supervisor. */
export async function adoptArchivedGoals(goalIds: readonly string[]): Promise<SupervisorCallResult> {
  try {
    const value = await invoke<unknown>("cmd_supervisor_adopt_archived", { goalIds: [...goalIds] });
    await refreshSupervisor();
    return { kind: "ok", value: value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {} };
  } catch (error) {
    const { code, message, status } = ipcRefusal(error);
    return { kind: "error", code, message, status };
  }
}

/** The last unified supervisor view this desktop saw, or null before the first one. */
export function getLastUnifiedSupervisor(): UnifiedSupervisorView | null {
  return lastUnified;
}

/** A refusal keeps its wire code; the UI branches on `code`, never on the message. */
async function desktopCall(call: () => Promise<unknown>): Promise<SupervisorCallResult> {
  try {
    const value = await call();
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      return { kind: "error", code: "INVALID_RESPONSE", message: "Supervisor response is not a JSON object", status: null };
    }
    return { kind: "ok", value: value as Record<string, unknown> };
  } catch (error) {
    const raw = (error && typeof error === "object" ? error : {}) as { code?: unknown; message?: unknown; details?: unknown };
    const details = raw.details && typeof raw.details === "object" ? (raw.details as { status?: unknown }) : null;
    return {
      kind: "error",
      code: typeof raw.code === "string" && raw.code ? raw.code : "INTERNAL_ERROR",
      message: typeof raw.message === "string" ? raw.message : String(error),
      status: typeof details?.status === "number" ? details.status : null,
    };
  }
}

/** The unified supervisor through this desktop's daemon, which owns the hub. */
export const desktopUnifiedClient: UnifiedSupervisorClient = {
  request: (envelope) => desktopCall(() => invoke<unknown>("cmd_supervisor_request", { envelope })),
  conversation: (projectId, after) =>
    desktopCall(() => invoke<unknown>("cmd_supervisor_read", { request: { view: "conversation", projectId, after } })),
  memory: () => desktopCall(() => invoke<unknown>("cmd_supervisor_read", { request: { view: "memory" } })),
  remember: (envelope) => desktopCall(() => invoke<unknown>("cmd_supervisor_request", { envelope })),
  refresh: () => void refreshSupervisor(),
};

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Snapshot access for non-React consumers (the Inbox hand-off recorder). */
export function getSupervisorSnapshot(): SupervisorSnapshot {
  return snapshot;
}

export function subscribeSupervisorSnapshot(listener: () => void): () => void {
  const unsubscribe = subscribe(listener);
  return () => { unsubscribe(); };
}

/** For tests and previews: replace the snapshot without the daemon. */
export function setSupervisorSnapshotForTest(next: SupervisorSnapshot) {
  publish(next);
}

/** For tests: forget the last unified view. */
export function resetLastUnifiedSupervisorForTest() {
  lastUnified = null;
}

/** The live snapshot, the last unified view seen, and a clock that ticks only when a derived state expires. */
export function useSupervisor(): { snapshot: SupervisorSnapshot; now: number; lastUnified: UnifiedSupervisorView | null } {
  const current = useSyncExternalStore(subscribe, () => snapshot, () => snapshot);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    void startSupervisorFeed();
  }, []);
  useEffect(() => {
    // A new snapshot restarts the clock once; the expiry timer below only advances it.
    setNow(Date.now());
  }, [current]);
  useEffect(() => {
    const expiry = nextSupervisionExpiry(current, now);
    if (expiry === null) return;
    const timer = setTimeout(() => setNow(Date.now()), expiry - now + 16);
    return () => clearTimeout(timer);
  }, [current, now]);
  return { snapshot: current, now, lastUnified };
}
