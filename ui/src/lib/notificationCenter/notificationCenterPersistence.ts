import { getMigratedItem, NOTIFICATION_HISTORY_STORAGE_KEY } from "../storageKeys";
import {
  capNotificationEntries, emptyInboxState, notificationEntryId,
  type InboxState, type NotificationEntry, type NotificationLabels,
} from "./types";

export interface NotificationCenterPersistenceOptions {
  storage?: Pick<Storage, "getItem" | "setItem"> | null;
  debounceMs?: number;
  onError?: (error: unknown) => void;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isTimestamp(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function isCounter(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

function restoreEntry(value: unknown): NotificationEntry | null {
  if (!isObject(value) || typeof value.workspaceId !== "string" || typeof value.sessionId !== "string"
    || value.id !== notificationEntryId(value.workspaceId, value.sessionId)
    || (value.subject !== "agent" && value.subject !== "terminal")
    || (value.reason !== "waiting" && value.reason !== "done" && value.reason !== "bell")
    || !isTimestamp(value.firstOccurredAt) || !isTimestamp(value.lastOccurredAt)
    || !isCounter(value.updateOrder) || !isCounter(value.revision) || !isCounter(value.occurrenceCount)
    || !isObject(value.labels) || !isObject(value.read)) return null;

  const labels: NotificationLabels = {};
  for (const key of ["workspaceLabel", "worktreeLabel", "agentLabel", "terminalTitle"] as const) {
    const label = value.labels[key];
    if (label !== undefined && typeof label !== "string") return null;
    if (typeof label === "string") labels[key] = label;
  }
  let read: NotificationEntry["read"];
  if (value.read.unread === true && value.read.seen === undefined) read = { unread: true };
  else if (value.read.seen === true && value.read.unread === undefined && isTimestamp(value.read.seenAt)) {
    read = { seen: true, seenAt: value.read.seenAt };
  } else return null;

  return {
    id: value.id, workspaceId: value.workspaceId, sessionId: value.sessionId,
    labels, subject: value.subject, reason: value.reason,
    firstOccurredAt: value.firstOccurredAt, lastOccurredAt: value.lastOccurredAt,
    updateOrder: value.updateOrder, revision: value.revision, occurrenceCount: value.occurrenceCount, read,
  };
}

/** Invalid rows are dropped; an invalid envelope restores an empty inbox. */
export function restoreNotificationInbox(raw: string | null): InboxState {
  if (raw === null) return emptyInboxState();
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return emptyInboxState();
  }
  if (!isObject(value) || value.version !== 1 || !Array.isArray(value.entries)) return emptyInboxState();
  const unique = new Map<string, NotificationEntry>();
  let nextUpdateOrder = isCounter(value.nextUpdateOrder) ? value.nextUpdateOrder : 1;
  for (const candidate of value.entries) {
    const entry = restoreEntry(candidate);
    if (!entry) continue;
    const existing = unique.get(entry.id);
    if (!existing || entry.updateOrder > existing.updateOrder) unique.set(entry.id, entry);
    nextUpdateOrder = Math.max(nextUpdateOrder, entry.updateOrder + 1);
  }
  return { version: 1, nextUpdateOrder, entries: capNotificationEntries([...unique.values()]) };
}

export function createNotificationCenterPersistence(options: NotificationCenterPersistenceOptions = {}) {
  let reportedError = false;
  const reportError = (error: unknown) => {
    if (reportedError) return;
    reportedError = true;
    if (options.onError) options.onError(error);
    else console.error("Notification history could not be persisted", error);
  };
  let storage: NotificationCenterPersistenceOptions["storage"] = null;
  try {
    storage = options.storage === undefined
      ? (typeof window === "undefined" ? null : window.localStorage)
      : options.storage;
  } catch (error) {
    reportError(error);
  }

  let initialState = emptyInboxState();
  if (storage) {
    const target = storage;
    try {
      initialState = restoreNotificationInbox(getMigratedItem(NOTIFICATION_HISTORY_STORAGE_KEY, {
        getItem: (key) => target.getItem(key),
        // getMigratedItem deliberately tolerates failed migration writes; still surface them.
        setItem: (key, value) => {
          try { target.setItem(key, value); }
          catch (error) { reportError(error); }
        },
      }));
    } catch (error) {
      reportError(error);
    }
  }

  let pending: InboxState | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const flush = () => {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    if (!pending || !storage) return;
    try {
      storage.setItem(NOTIFICATION_HISTORY_STORAGE_KEY, JSON.stringify(pending));
      pending = undefined;
    } catch (error) {
      // Keep the pending snapshot, allowing explicit flush or a later occurrence to retry.
      reportError(error);
    }
  };
  const schedule = (state: InboxState) => {
    if (!storage) return;
    pending = state;
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(flush, options.debounceMs ?? 250);
  };
  const onVisibilityChange = () => {
    if (document.visibilityState === "hidden") flush();
  };
  if (storage && typeof window !== "undefined") {
    window.addEventListener("pagehide", flush);
    document.addEventListener("visibilitychange", onVisibilityChange);
  }
  const dispose = () => {
    flush();
    if (typeof window !== "undefined") {
      window.removeEventListener("pagehide", flush);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    }
  };
  return { initialState, schedule, flush, dispose };
}
