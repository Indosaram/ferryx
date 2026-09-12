export interface NotificationLabels {
  workspaceLabel?: string;
  worktreeLabel?: string;
  agentLabel?: string;
  terminalTitle?: string;
}

export type NotificationReadState = { unread: true } | { seen: true; seenAt: number };

export interface NotificationEntry {
  id: string;
  workspaceId: string;
  sessionId: string;
  labels: NotificationLabels;
  subject: "agent" | "terminal";
  reason: "waiting" | "done" | "bell";
  firstOccurredAt: number;
  lastOccurredAt: number;
  updateOrder: number;
  revision: number;
  occurrenceCount: number;
  read: NotificationReadState;
}

export interface InboxState {
  version: 1;
  nextUpdateOrder: number;
  entries: NotificationEntry[];
}

export interface BellOccurrence {
  workspaceId: string;
  sessionId: string;
  labels: NotificationLabels;
  subject: NotificationEntry["subject"];
  /** Source occurrence time, not the time a duplicate callback happens to run. */
  occurredAt: number;
  /** Prefer a stable source-event ID when multiple events can share a timestamp. */
  occurrenceId?: string;
  /** Focused AND actively observing this session at occurrence time. */
  observed: boolean;
}

/** Only coordinator-accepted edges should reach recordActivity. */
export interface ActivityOccurrence extends BellOccurrence {
  previousState?: string;
  state: string;
  notificationSuppressed?: boolean;
}

export interface ReadAcknowledgement {
  id: string;
  expectedRevision: number;
}

export const MAX_NOTIFICATION_ENTRIES = 200;

export function notificationEntryId(workspaceId: string, sessionId: string): string {
  return JSON.stringify(["session", workspaceId, sessionId]);
}

export function emptyInboxState(): InboxState {
  return { version: 1, nextUpdateOrder: 1, entries: [] };
}

/** Display newest first, but preferentially evict the oldest already-seen row. */
export function capNotificationEntries(entries: NotificationEntry[]): NotificationEntry[] {
  const sorted = [...entries].sort((a, b) => b.updateOrder - a.updateOrder);
  while (sorted.length > MAX_NOTIFICATION_ENTRIES) {
    let oldestSeen = -1;
    for (let index = sorted.length - 1; index >= 0; index--) {
      if ("seen" in sorted[index].read) {
        oldestSeen = index;
        break;
      }
    }
    sorted.splice(oldestSeen === -1 ? sorted.length - 1 : oldestSeen, 1);
  }
  return sorted;
}
