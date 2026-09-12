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

/** Mirrors workspaceStore's private isSessionActivelyObserved selector (store is read-only here). */
export function isNotificationTargetObserved(state: WorkspaceState, target: RecordingTarget, focused: boolean): boolean {
  if (!focused || target.workspaceId !== state.workspaceId) return false;
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
