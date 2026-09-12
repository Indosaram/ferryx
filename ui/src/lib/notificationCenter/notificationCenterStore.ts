import {
  createNotificationCenterPersistence,
  type NotificationCenterPersistenceOptions,
} from "./notificationCenterPersistence";
import {
  capNotificationEntries, MAX_NOTIFICATION_ENTRIES, notificationEntryId,
  type ActivityOccurrence, type BellOccurrence, type InboxState,
  type NotificationEntry, type ReadAcknowledgement,
} from "./types";

export type NotificationCenterOptions = NotificationCenterPersistenceOptions;

export function createNotificationCenterStore(options: NotificationCenterOptions = {}) {
  const persistence = createNotificationCenterPersistence(options);
  let state = persistence.initialState;
  const listeners = new Set<() => void>();
  // A bounded replay guard, not another session-state classifier. Callers own edge acceptance.
  const recentOccurrences = new Set<string>();

  const publish = (next: InboxState) => {
    state = next;
    persistence.schedule(state);
    listeners.forEach((listener) => listener());
  };

  const record = (occurrence: BellOccurrence, reason: NotificationEntry["reason"], edge: string): boolean => {
    const id = notificationEntryId(occurrence.workspaceId, occurrence.sessionId);
    const occurrenceKey = JSON.stringify([id, occurrence.occurrenceId === undefined
      ? [edge, occurrence.occurredAt] : [occurrence.occurrenceId]]);
    if (recentOccurrences.has(occurrenceKey)) return false;
    recentOccurrences.add(occurrenceKey);
    if (recentOccurrences.size > MAX_NOTIFICATION_ENTRIES * 2) {
      recentOccurrences.delete(recentOccurrences.values().next().value!);
    }
    const previous = state.entries.find((entry) => entry.id === id);
    const entry: NotificationEntry = {
      id, workspaceId: occurrence.workspaceId, sessionId: occurrence.sessionId,
      labels: { ...occurrence.labels }, subject: occurrence.subject, reason,
      firstOccurredAt: previous?.firstOccurredAt ?? occurrence.occurredAt,
      lastOccurredAt: occurrence.occurredAt,
      updateOrder: state.nextUpdateOrder,
      revision: (previous?.revision ?? 0) + 1,
      occurrenceCount: (previous?.occurrenceCount ?? 0) + 1,
      read: occurrence.observed ? { seen: true, seenAt: occurrence.occurredAt } : { unread: true },
    };
    publish({ version: 1, nextUpdateOrder: state.nextUpdateOrder + 1,
      entries: capNotificationEntries([entry, ...state.entries.filter((value) => value.id !== id)]),
    });
    return true;
  };

  const markEntriesRead = (acknowledgements: ReadAcknowledgement[], seenAt = Date.now()) => {
    const revisions = new Map(acknowledgements.map((ack) => [ack.id, ack.expectedRevision]));
    let changed = false;
    const entries = state.entries.map((entry): NotificationEntry => {
      const expectedRevision = revisions.get(entry.id);
      if (expectedRevision === undefined || expectedRevision !== entry.revision || "seen" in entry.read) return entry;
      changed = true;
      return { ...entry, read: { seen: true, seenAt } };
    });
    if (changed) publish({ ...state, entries });
  };

  return {
    getSnapshot: () => state,
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    recordActivity: (occurrence: ActivityOccurrence): boolean => {
      if (occurrence.notificationSuppressed || occurrence.previousState === undefined
        || occurrence.previousState === occurrence.state
        || (occurrence.state !== "waiting" && occurrence.state !== "done")) return false;
      return record(occurrence, occurrence.state, JSON.stringify([occurrence.previousState, occurrence.state]));
    },
    /** Only bells accepted past coordinator throttle/post-completion suppression reach here. */
    recordBell: (occurrence: BellOccurrence) => record(occurrence, "bell", "bell"),
    markEntriesRead,
    markAllRead: (seenAt = Date.now()) => {
      markEntriesRead(state.entries.map((entry) => ({ id: entry.id, expectedRevision: entry.revision })), seenAt);
    },
    dismissEntry: (id: string) => {
      const entries = state.entries.filter((entry) => entry.id !== id);
      if (entries.length !== state.entries.length) publish({ ...state, entries });
    },
    clearAll: () => {
      if (state.entries.length > 0) publish({ ...state, entries: [] });
    },
    flush: persistence.flush,
    dispose: () => {
      persistence.dispose();
      listeners.clear();
    },
  };
}

export type NotificationCenterStore = ReturnType<typeof createNotificationCenterStore>;
export const notificationCenterStore = createNotificationCenterStore();
