import { useSyncExternalStore } from "react";
import {
  notificationCenterStore,
  type NotificationCenterStore,
} from "../../lib/notificationCenter/notificationCenterStore";

export function useNotificationCenter(store: NotificationCenterStore = notificationCenterStore) {
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const unreadEntries = state.entries.filter((entry) => "unread" in entry.read);
  const unreadCount = unreadEntries.length;

  return {
    state,
    entries: unreadEntries,
    unreadCount,
    markEntriesRead: store.markEntriesRead,
    markAllRead: store.markAllRead,
    dismissEntry: store.dismissEntry,
    clearAll: store.clearAll,
  };
}
