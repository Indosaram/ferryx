import { invoke } from "@tauri-apps/api/core";

export type AgentHistoryProvider = "claude" | "codex";

export type AgentHistoryEntry = {
  entryKey: string;
  provider: "claude" | "codex";
  providerSession: { key: "session_id" | "conversation_id"; id: string; transcriptPath?: string | null };
  cwd: string;
  version?: string | null;
  parentId?: string | null;
  modifiedMs?: number | null;
};

export type AgentHistoryMessage = {
  ordinal: number;
  role: string;
  text: string;
  id?: string | null;
  parentId?: string | null;
};

export type AgentHistoryPage<T> = {
  items: T[];
  nextCursor: string | null;
  partial: boolean;
  warnings: string[];
};

export async function searchAgentHistory(request: {
  provider: AgentHistoryProvider;
  cwd: string | null;
  query: string;
  cursor: string | null;
  limit: number;
}): Promise<AgentHistoryPage<AgentHistoryEntry>> {
  return await invoke<AgentHistoryPage<AgentHistoryEntry>>("cmd_agent_history_search", request);
}

export async function readAgentHistory(request: {
  entryKey: string;
  cursor: string | null;
  limit: number;
}): Promise<AgentHistoryPage<AgentHistoryMessage>> {
  return await invoke<AgentHistoryPage<AgentHistoryMessage>>("cmd_agent_history_read", request);
}
