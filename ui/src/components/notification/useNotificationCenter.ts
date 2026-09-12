import { useSyncExternalStore } from "react";
import {
  notificationCenterStore,
  type NotificationCenterStore,
} from "../../lib/notificationCenter/notificationCenterStore";

export function useNotificationCenter(store: NotificationCenterStore = notificationCenterStore) {
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const unreadCount = state.entries.filter((entry) => "unread" in entry.read).length;

  return {
    state,
    entries: state.entries,
    unreadCount,
    markEntriesRead: store.markEntriesRead,
    markAllRead: store.markAllRead,
    dismissEntry: store.dismissEntry,
    clearAll: store.clearAll,
  };
}
