import { referenceNativeKindFromRegistryId, referenceNativeKindIsNative } from "../remote/chat/referenceTypes";
import { getMigratedItem } from "./storageKeys";
import { ANNOUNCE_LEVELS, isAnnounceLevel, type AnnounceLevel } from "../remote/chat/messages/announcer";

export const PANE_CHAT_LAST_AGENT_STORAGE_KEY = "ferryx.paneChat.lastAgent";
/** Tool calls in chat (t3code Settings -> Chat). Device-local, like the reference. */
export const PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY = "ferryx.paneChat.toolCallVisibility";
/** Screen reader announcements (t3code Settings -> Chat). Device-local, like the reference. */
export const PANE_CHAT_ANNOUNCE_LEVEL_STORAGE_KEY = "ferryx.paneChat.announceLevel";
/** Fires in this window when either chat preference changes, so a mounted pane re-reads it. */
export const PANE_CHAT_PREFERENCES_EVENT = "ferryx-pane-chat-preferences";

export type ToolCallVisibility = "all" | "hide-successful" | "hide-all" | "zen";

export const TOOL_CALL_VISIBILITIES: readonly ToolCallVisibility[] = [
  "all",
  "hide-successful",
  "hide-all",
  "zen",
];

export function isToolCallVisibility(value: unknown): value is ToolCallVisibility {
  return (
    typeof value === "string" && (TOOL_CALL_VISIBILITIES as readonly string[]).includes(value)
  );
}

export function loadToolCallVisibility(store: Storage | null = storage()): ToolCallVisibility {
  try {
    const value = store?.getItem(PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY);
    return isToolCallVisibility(value) ? value : "all";
  } catch {
    return "all";
  }
}

export function saveToolCallVisibility(
  visibility: ToolCallVisibility,
  store: Storage | null = storage(),
): void {
  try {
    store?.setItem(PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY, visibility);
    announcePaneChatPreferenceChange();
  } catch (error) {
    console.warn("Could not persist tool call visibility", error);
  }
}

/**
 * Screen reader announcements for the open thread (activity-log.md "Screen reader announcements").
 * The reference's default is **Status and messages** on web and desktop, which is what an absent
 * value means here: a reader who never opens Settings still hears a turn start and finish.
 */
export const DEFAULT_ANNOUNCE_LEVEL: AnnounceLevel = "status-and-messages";

export function loadAnnounceLevel(store: Storage | null = storage()): AnnounceLevel {
  try {
    const value = store?.getItem(PANE_CHAT_ANNOUNCE_LEVEL_STORAGE_KEY);
    return isAnnounceLevel(value) ? value : DEFAULT_ANNOUNCE_LEVEL;
  } catch {
    return DEFAULT_ANNOUNCE_LEVEL;
  }
}

export function saveAnnounceLevel(
  level: AnnounceLevel,
  store: Storage | null = storage(),
): void {
  try {
    store?.setItem(PANE_CHAT_ANNOUNCE_LEVEL_STORAGE_KEY, level);
    announcePaneChatPreferenceChange();
  } catch (error) {
    console.warn("Could not persist screen reader announcement level", error);
  }
}

function announcePaneChatPreferenceChange(): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(PANE_CHAT_PREFERENCES_EVENT));
}

/**
 * Re-read the chat preferences whenever they change: in this window from the Settings control,
 * and in another window through the `storage` event the browser sends.
 */
export function subscribePaneChatPreferences(listener: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  const onStorage = (event: StorageEvent) => {
    if (
      event.key === null ||
      event.key === PANE_CHAT_TOOL_CALL_VISIBILITY_STORAGE_KEY ||
      event.key === PANE_CHAT_ANNOUNCE_LEVEL_STORAGE_KEY
    ) {
      listener();
    }
  };
  window.addEventListener(PANE_CHAT_PREFERENCES_EVENT, listener);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(PANE_CHAT_PREFERENCES_EVENT, listener);
    window.removeEventListener("storage", onStorage);
  };
}

export { ANNOUNCE_LEVELS };

export type ChatAgent = { name: string; command: string; args: string };

// Chat needs a native history reader; registry ids without one map to "unavailable".
export function isChatSupportedAgent(registryId: string | null | undefined): boolean {
  if (!registryId) return false;
  return referenceNativeKindIsNative(referenceNativeKindFromRegistryId(registryId));
}

export function chatSupportedAgents<T extends ChatAgent>(agents: readonly T[]): T[] {
  return agents.filter((agent) => isChatSupportedAgent(agent.name));
}

function storage(): Storage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadLastChatAgentId(store: Storage | null = storage()): string | null {
  try {
    const value = getMigratedItem(PANE_CHAT_LAST_AGENT_STORAGE_KEY, store);
    return value && value.trim() ? value : null;
  } catch {
    return null;
  }
}

export function saveLastChatAgentId(agentId: string, store: Storage | null = storage()): void {
  try {
    store?.setItem(PANE_CHAT_LAST_AGENT_STORAGE_KEY, agentId);
  } catch (error) {
    console.warn("Could not persist last chat agent", error);
  }
}

export function orderChatAgents<T extends ChatAgent>(
  agents: readonly T[],
  lastAgentId: string | null,
  defaultAgentId: string | null | undefined,
): T[] {
  const rank = (agent: T) => (agent.name === lastAgentId ? 0 : agent.name === defaultAgentId ? 1 : 2);
  return agents
    .map((agent, index) => ({ agent, index }))
    .sort((a, b) => rank(a.agent) - rank(b.agent) || a.index - b.index)
    .map(({ agent }) => agent);
}

export function pickChatAgent<T extends ChatAgent>(
  agents: readonly T[],
  lastAgentId: string | null,
  defaultAgentId: string | null | undefined,
): T | null {
  return orderChatAgents(agents, lastAgentId, defaultAgentId)[0] ?? null;
}
