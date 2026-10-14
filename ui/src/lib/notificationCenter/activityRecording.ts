import type { ActivityNotificationEvent, ActivityNotificationTarget, WorkspaceState } from "../../state/workspaceStore";
import type { NotificationDecision } from "../notificationCoordinator";
import { notificationCenterStore, type NotificationCenterStore } from "./notificationCenterStore";

export type RecordingTarget = Pick<ActivityNotificationTarget,
  "workspaceId" | "workspaceLabel" | "sessionId" | "tabId" | "worktreeLabel" | "agentLabel" | "terminalTitle">;

type RecordedEvent<T> = T & { occurredAt?: number; occurrenceId?: string };
export type RecordingListener<T> = (event: RecordedEvent<T>, decision: NotificationDecision) => void;
interface RecordingOptions<T> {
  /** The caller invokes the coordinator first and supplies its pre-focus decision. */
  events: (listener: RecordingListener<T>) => () => void;
  isObserved: (target: T) => boolean;
  store?: NotificationCenterStore;
}

function occurrence(target: RecordedEvent<RecordingTarget>, observed: boolean) {
  return {
    workspaceId: target.workspaceId!, sessionId: target.sessionId,
    labels: {
      workspaceLabel: target.workspaceLabel, worktreeLabel: target.worktreeLabel,
      agentLabel: target.agentLabel, terminalTitle: target.terminalTitle,
    },
    subject: target.agentLabel ? "agent" as const : "terminal" as const,
    occurredAt: target.occurredAt ?? Date.now(), occurrenceId: target.occurrenceId, observed,
  };
}

export function wireActivityRecording({ events, isObserved, store = notificationCenterStore }: RecordingOptions<ActivityNotificationEvent>): () => void {
  return events((event, decision) => {
    if (!decision.accepted || !event.workspaceId) return;
    store.recordActivity({ ...occurrence(event, isObserved(event)),
      subject: "agent",
      previousState: event.previousState, state: event.state,
      notificationSuppressed: event.notificationSuppressed,
    });
  });
}

export function wireBellRecording({ events, isObserved, store = notificationCenterStore }: RecordingOptions<RecordingTarget>): () => void {
  return events((event, decision) => {
    if (!decision.accepted || !event.workspaceId) return;
    store.recordBell(occurrence(event, isObserved(event)));
  });
}

/**
 * How recently the user must have actually touched the app for a surfaced event to count as
 * observed. Window focus alone is not observation: the window can hold OS focus while the user
 * reads on a second monitor or is away from the desk, and marking a completion seen in that state
 * silently swallows the notification, the sound, and the unread badge.
 */
const OBSERVATION_INTERACTION_WINDOW_MS = 30_000;

let lastInteractionAt = 0;

/** Test seam: record an interaction at an explicit time, or reset the clock with 0. */
export function noteObservationInteraction(at: number = Date.now()): void {
  lastInteractionAt = at;
}

/** Whether a human touched the app recently enough for a surfaced event to count as observed. */
export function hasRecentObservationInteraction(now: number = Date.now()): boolean {
  return now - lastInteractionAt <= OBSERVATION_INTERACTION_WINDOW_MS;
}

if (typeof window !== "undefined") {
  const note = () => {
    lastInteractionAt = Date.now();
  };
  const types = ["keydown", "pointerdown", "wheel"];
  for (const type of types) {
    window.addEventListener(type, note, { capture: true, passive: true });
  }
  // Without this each HMR re-evaluation stacks another set of never-removed listeners.
  import.meta.hot?.dispose(() => {
    for (const type of types) window.removeEventListener(type, note, { capture: true });
  });
}

/** Mirrors workspaceStore's private isSessionActivelyObserved selector (store is read-only here). */
export function isNotificationTargetObserved(state: WorkspaceState, target: RecordingTarget, focused: boolean): boolean {
  if (!focused || target.workspaceId !== state.workspaceId) return false;
  // Focus says the window is frontmost; interaction recency says a human is actually there.
  if (!hasRecentObservationInteraction()) return false;
  const visible = state.layout.activeTabId === target.tabId ||
    Object.values(state.layout.tabGroups ?? {}).some((group) => group.activeTabId === target.tabId);
  if (!visible) return false;
  const layout = state.layout.layoutsByTabId?.[target.tabId];
  if (layout?.root.type === "split") {
    const leafId = Object.entries(layout.sessionIdsByLeafId).find(([, id]) => id === target.sessionId)?.[0];
    return Boolean(leafId && leafId === layout.activeLeafId);
  }
  return true;
}
