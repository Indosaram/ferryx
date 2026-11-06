import { referenceNativeKindFromRegistryId, referenceNativeKindIsNative } from "../remote/chat/referenceTypes";
import { getMigratedItem } from "./storageKeys";

export const PANE_CHAT_LAST_AGENT_STORAGE_KEY = "ferryx.paneChat.lastAgent";

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
