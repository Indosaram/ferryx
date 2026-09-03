import type { AgentProviderSession, AgentProviderSessionKey } from "./types";
export type ResumeCapability = "uuid" | "positional-index" | "chat-id" | "none";

export type AgentSessionRef = {
  readonly agentType: string;
  readonly providerSession?: AgentProviderSession | null;
  /** @deprecated Legacy fallback only */
  readonly sessionId?: string | null;
};

export const PROVIDER_SESSION_ID_MAX_LENGTH = 512;
export const PROVIDER_TRANSCRIPT_PATH_MAX_LENGTH = 4096;

export const RESUMABLE_TUI_AGENTS = [
  "claude", "codex", "antigravity", "opencode", "pi", "prime-agent",
  "mimo-code", "droid", "grok", "devin", "omp", "omo", "kimi", "gjc",
  "copilot", "cursor", "cursor-agent",
] as const;

export type ResumableTuiAgent = (typeof RESUMABLE_TUI_AGENTS)[number];

type ResumeArgvBuilder = {
  readonly key: AgentProviderSessionKey;
  readonly binary: string;
  readonly args: (id: string, s: AgentProviderSession, ompPath?: string) => string[];
};

const RESUME_BUILDERS: Readonly<Record<string, ResumeArgvBuilder>> = {
  claude: { key: "session_id", binary: "claude", args: (id) => ["--resume", id] },
  codex: { key: "session_id", binary: "codex", args: (id) => ["resume", id] },
  antigravity: { key: "session_id", binary: "agy", args: (id) => ["--conversation", id] },
  opencode: { key: "session_id", binary: "opencode", args: (id) => ["--session", id] },
  pi: { key: "session_id", binary: "pi", args: (_id, s) => (s.transcriptPath ? ["--session", s.transcriptPath] : []) },
  "prime-agent": { key: "session_id", binary: "prime-agent", args: (_id, s) => (s.transcriptPath ? ["--resume", s.transcriptPath] : []) },
  "mimo-code": { key: "session_id", binary: "mimo", args: (id) => ["--session", id] },
  droid: { key: "session_id", binary: "droid", args: (id) => ["--resume", id] },
  grok: { key: "session_id", binary: "grok", args: (id) => ["--resume", id] },
  devin: { key: "session_id", binary: "devin", args: (id) => ["--resume", id] },
  omp: { key: "session_id", binary: "omp", args: (id, _s, p) => ["--resume", p?.trim() || id] },
  omo: { key: "session_id", binary: "omo", args: (id) => ["--session", id] },
  kimi: { key: "session_id", binary: "kimi", args: (id) => ["--session", id] },
  gjc: { key: "session_id", binary: "gjc", args: (id) => ["--resume", id] },
  copilot: { key: "session_id", binary: "copilot", args: (id) => ["--resume", id] },
  cursor: { key: "session_id", binary: "cursor-agent", args: (id) => ["--resume", id] },
  "cursor-agent": { key: "session_id", binary: "cursor-agent", args: (id) => ["--resume", id] },
};

export function providerSessionKeyForAgent(agentType: string): AgentProviderSessionKey | null {
  return RESUME_BUILDERS[agentType.trim().toLowerCase()]?.key ?? null;
}
export const AUTHORITATIVE_RECONNECT_AGENTS = [
  "claude",
  "codex",
  "copilot",
  "cursor",
  "cursor-agent",
  "kimi",
  "omo",
  "gjc",
  "antigravity",
  "opencode",
  "pi",
] as const;

export function canCaptureAuthoritativeProviderSession(agentType: string): boolean {
  return (AUTHORITATIVE_RECONNECT_AGENTS as readonly string[]).includes(agentType.trim().toLowerCase());
}

export function hasUnsafeProviderSessionIdChars(value: string): boolean {
  for (let i = 0; i < value.length; i += 1) {
    const code = value.charCodeAt(i);
    if (code <= 0x1f || code === 0x7f) return true;
  }
  return false;
}

export function normalizeSessionId(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  if (
    trimmed.length === 0 ||
    trimmed.length > PROVIDER_SESSION_ID_MAX_LENGTH ||
    trimmed.startsWith("-") ||
    trimmed.toLowerCase() === "latest" ||
    hasUnsafeProviderSessionIdChars(trimmed)
  ) {
    return null;
  }
  return trimmed;
}

function readSessionId(record: Record<string, unknown>, keys: readonly string[]): string | null {
  for (const key of keys) {
    const normalized = normalizeSessionId(record[key]);
    if (normalized) return normalized;
  }
  return null;
}

function readTranscriptPath(record: Record<string, unknown>, keys: readonly string[]): string | undefined {
  for (const key of keys) {
    const raw = record[key];
    if (typeof raw !== "string") continue;
    const trimmed = raw.trim();
    if (
      trimmed.length > 0
      && trimmed.length <= PROVIDER_TRANSCRIPT_PATH_MAX_LENGTH
      && !trimmed.startsWith("-")
      && !hasUnsafeProviderSessionIdChars(trimmed)
      && (/^\//.test(trimmed) || /^[A-Za-z]:[\\/]/.test(trimmed) || /^\\\\[^\\]/.test(trimmed))
    ) return trimmed;
  }
  return undefined;
}

export function normalizeAgentProviderSession(raw: unknown): AgentProviderSession | null {
  if (typeof raw !== "object" || raw === null) return null;
  const record = raw as Record<string, unknown>;
  const key = record.key;
  if (key !== "session_id" && key !== "conversation_id") return null;
  const id = normalizeSessionId(record.id);
  if (!id) return null;
  const transcriptPath = readTranscriptPath(record, ["transcriptPath", "transcript_path"]);
  return transcriptPath ? { key, id, transcriptPath } : { key, id };
}

export function agentProviderSessionsEqual(
  agent: string,
  left?: AgentProviderSession | null,
  right?: AgentProviderSession | null,
): boolean {
  if (left === undefined || left === null || right === undefined || right === null) {
    return left === right;
  }
  const samePath = agent !== "pi" && agent !== "prime-agent" ? true : left.transcriptPath === right.transcriptPath;
  return left.key === right.key && left.id === right.id && samePath;
}

function normalizeAgentType(agentType?: string | null): string {
  return agentType ? agentType.trim().toLowerCase() : "";
}

export function isResumableTuiAgent(value: unknown): value is ResumableTuiAgent {
  return typeof value === "string" && normalizeAgentType(value) in RESUME_BUILDERS;
}

export function agentResumeCapability(agentType: string): ResumeCapability {
  return isResumableTuiAgent(agentType) ? "uuid" : "none";
}

export function canResumeAgent(agentType: string): boolean {
  return isResumableTuiAgent(agentType);
}

export function extractAgentProviderSession(
  source: string,
  payload: Record<string, unknown>,
): AgentProviderSession | null {
  const norm = normalizeAgentType(source);
  if (norm === "pi" || norm === "prime-agent") {
    const id = readSessionId(payload, ["session_id"]);
    const transcriptPath = readTranscriptPath(payload, ["session_file", "transcript_path", "transcriptPath"]);
    return id && transcriptPath ? { key: "session_id", id, transcriptPath } : null;
  }
  if (norm in RESUME_BUILDERS) {
    const id = readSessionId(payload, ["session_id", "sessionId", "sessionID"]);
    if (!id) return null;
    const transcriptPath = readTranscriptPath(payload, ["transcript_path", "transcriptPath"]);
    return transcriptPath ? { key: "session_id", id, transcriptPath } : { key: "session_id", id };
  }
  return null;
}

export function getAgentResumeArgv(
  agent: string,
  providerSession: AgentProviderSession,
  ompResumeFilePath?: string,
): string[] | null {
  const normSession = normalizeAgentProviderSession(providerSession);
  if (!normSession) return null;
  const builder = RESUME_BUILDERS[normalizeAgentType(agent)];
  if (!builder || builder.key !== normSession.key) return null;
  const args = builder.args(normSession.id, normSession, ompResumeFilePath);
  return args.length > 0 ? [builder.binary, ...args] : null;
}

export function buildResumeArgv(ref: AgentSessionRef): string[] | null {
  if (!ref) return null;
  const normAgent = normalizeAgentType(ref.agentType);
  if (!normAgent) return null;

  let session = ref.providerSession ? normalizeAgentProviderSession(ref.providerSession) : null;
  if (!session && typeof ref.sessionId === "string") {
    session = normalizeAgentProviderSession({ key: "session_id", id: ref.sessionId });
  }
  if (!session) return null;

  const argv = getAgentResumeArgv(normAgent, session);
  if (!argv) return null;

  return argv;
}
