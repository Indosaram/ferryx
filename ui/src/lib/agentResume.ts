export type ResumeCapability = "uuid" | "positional-index" | "chat-id" | "none";

export type AgentSessionRef = {
  agentType: string;
  sessionId: string;
};

export type AgentResumeSpec = {
  capability: ResumeCapability;
  defaultCommand: string;
  args: (sessionId: string) => readonly string[];
};

export const AGENT_RESUME_SPECS: Readonly<Record<string, AgentResumeSpec>> = {
  claude: {
    capability: "uuid",
    defaultCommand: "claude",
    args: (id) => ["--resume", id],
  },
  codex: {
    capability: "uuid",
    defaultCommand: "codex",
    args: (id) => ["resume", id],
  },
  copilot: {
    capability: "uuid",
    defaultCommand: "copilot",
    args: (id) => ["--resume", id],
  },
  kimi: {
    capability: "uuid",
    defaultCommand: "kimi",
    args: (id) => ["--session", id],
  },
  opencode: {
    capability: "uuid",
    defaultCommand: "opencode",
    // Top-level `opencode [project]` is the TUI (the default command); `run` is the
    // non-interactive "run with a message" path and would exit immediately in a pane.
    args: (id) => ["-s", id],
  },
  gemini: {
    capability: "positional-index",
    defaultCommand: "gemini",
    args: (id) => ["-r", id],
  },
  cursor: {
    capability: "chat-id",
    defaultCommand: "cursor-agent",
    args: (id) => ["--resume", id],
  },
  omo: {
    capability: "uuid",
    defaultCommand: "omo",
    // `--resume`/-r takes no value here (it opens an interactive picker), so passing an id
    // there would be parsed as a positional prompt and start a NEW session. `--session
    // <path|id>` resumes a specific one. Never `--session-id`, which mints when missing.
    args: (id) => ["--session", id],
  },
  grok: {
    capability: "none",
    defaultCommand: "grok",
    args: () => [],
  },
  antigravity: {
    capability: "none",
    defaultCommand: "antigravity",
    args: () => [],
  },
  pi: {
    capability: "none",
    defaultCommand: "pi",
    args: () => [],
  },
  cline: {
    capability: "none",
    defaultCommand: "cline",
    args: () => [],
  },
} as const;

function normalizeAgentType(agentType?: string | null): string {
  if (!agentType) return "";
  return agentType.trim().toLowerCase();
}

export function agentResumeCapability(agentType: string): ResumeCapability {
  const normalized = normalizeAgentType(agentType);
  if (!normalized) return "none";
  return AGENT_RESUME_SPECS[normalized]?.capability ?? "none";
}

export function canResumeAgent(agentType: string): boolean {
  return agentResumeCapability(agentType) !== "none";
}

export function buildResumeArgv(ref: AgentSessionRef, baseCommand?: string): string[] | null {
  if (!ref) return null;
  const normalizedType = normalizeAgentType(ref.agentType);
  if (!normalizedType) return null;

  const trimmedSessionId = ref.sessionId ? ref.sessionId.trim() : "";
  if (!trimmedSessionId) return null;

  const spec = AGENT_RESUME_SPECS[normalizedType];
  if (!spec || spec.capability === "none") return null;

  const trimmedBaseCommand = baseCommand ? baseCommand.trim() : "";
  const command = trimmedBaseCommand || spec.defaultCommand;

  return [command, ...spec.args(trimmedSessionId)];
}
