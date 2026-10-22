import { invoke } from "@tauri-apps/api/core";

// Mirrors `CanonicalProvider` in `src-tauri/src/scoped_contracts.rs`; every entry has a history
// root in `provider_history_roots` and a reader in `ferryx_scope/history/`.
export const AGENT_HISTORY_PROVIDERS = [
  "claude",
  "codex",
  "omo",
  "pi",
  "omp",
  "gjc",
  "antigravity",
  "opencode",
  "copilot",
  "cursor",
  "kimi",
  "cline",
  "grok",
  "droid",
  "crush",
] as const;

export type AgentHistoryProvider = (typeof AGENT_HISTORY_PROVIDERS)[number];

const AGENT_HISTORY_PROVIDER_LABELS: Readonly<Record<AgentHistoryProvider, string>> = {
  claude: "Claude",
  codex: "Codex",
  omo: "omo",
  pi: "pi",
  omp: "omp",
  gjc: "gjc",
  antigravity: "Antigravity",
  opencode: "OpenCode",
  copilot: "Copilot",
  cursor: "Cursor",
  kimi: "Kimi",
  cline: "Cline",
  grok: "Grok",
  droid: "Droid",
  crush: "Crush",
};

export function agentHistoryProviderLabel(provider: string): string {
  return AGENT_HISTORY_PROVIDER_LABELS[provider as AgentHistoryProvider] ?? provider;
}

export type AgentHistoryEntry = {
  entryKey: string;
  provider: AgentHistoryProvider;
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
