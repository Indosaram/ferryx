import {
  agentProviderSessionsEqual,
  buildResumeArgv,
  canCaptureAuthoritativeProviderSession,
  canResumeAgent,
  normalizeAgentProviderSession,
} from "./agentResume";
import type {
  AgentProviderSession,
  ReconnectLifecycle,
  StructuredIpcError,
  TerminalSession,
} from "./types";

export type ResumableAgentPane = {
  readonly sessionId: string;
  readonly agentType: string;
  readonly agentSessionId: string;
  readonly providerSession?: AgentProviderSession;
  readonly cwd: string;
  readonly argv: string[];
};

export type AgentReconnectStatus =
  | "idle"
  | "reconnecting"
  | "failed"
  | "conflict"
  | "unsupported"
  | "missing_reference"
  | "none";

export type AgentReconnectAffordance = {
  readonly status: AgentReconnectStatus;
  readonly canReconnect: boolean;
  readonly canRetry: boolean;
  readonly isReconnecting: boolean;
  readonly sessionId: string;
  readonly agentType: string | null;
  readonly providerSession: AgentProviderSession | null;
  readonly reconnectLifecycle: ReconnectLifecycle;
  readonly error: StructuredIpcError | null;
  readonly reason: string | null;
  readonly conflictingSessionId: string | null;
  readonly argv: string[] | null;
  readonly cwd?: string;
};

function affordance(
  status: AgentReconnectStatus,
  session: TerminalSession,
  overrides: Partial<AgentReconnectAffordance> = {},
): AgentReconnectAffordance {
  return {
    status,
    canReconnect: status === "idle" || status === "failed",
    canRetry: status === "failed",
    isReconnecting: status === "reconnecting",
    sessionId: session.id,
    agentType: session.agentType?.trim() || null,
    providerSession: overrides.providerSession ?? null,
    reconnectLifecycle: overrides.reconnectLifecycle ?? session.reconnectLifecycle ?? "idle",
    error: overrides.error ?? null,
    reason: overrides.reason ?? null,
    conflictingSessionId: overrides.conflictingSessionId ?? null,
    argv: overrides.argv ?? null,
    cwd: session.cwd,
    ...overrides,
  };
}

export function extractNormalizedProviderSession(session: TerminalSession): AgentProviderSession | null {
  if (session.providerSession) {
    return normalizeAgentProviderSession(session.providerSession);
  }
  if (typeof session.agentSessionId === "string" && session.agentSessionId.trim().length > 0) {
    const rawAgent = session.agentType?.trim().toLowerCase() ?? "";
    const key = rawAgent === "antigravity" ? "conversation_id" : "session_id";
    return normalizeAgentProviderSession({ key, id: session.agentSessionId });
  }
  return null;
}

export function findConflictingActiveSession(
  session: TerminalSession,
  allSessions?: Readonly<Record<string, TerminalSession>> | readonly TerminalSession[],
): TerminalSession | null {
  if (!allSessions || !session.agentType) return null;
  const targetNorm = extractNormalizedProviderSession(session);
  if (!targetNorm) return null;

  const sessionList = Array.isArray(allSessions) ? allSessions : Object.values(allSessions);
  const targetAgent = session.agentType.trim().toLowerCase();

  for (const candidate of sessionList) {
    if (candidate.id === session.id) continue;
    if (!candidate.agentType || candidate.agentType.trim().toLowerCase() !== targetAgent) continue;
    const candidateNorm = extractNormalizedProviderSession(candidate);
    if (!candidateNorm || !agentProviderSessionsEqual(targetAgent, targetNorm, candidateNorm)) continue;

    const isLive = candidate.backendSessionId !== null || candidate.lifecycle !== "exited";
    const isReconnecting =
      candidate.reconnectLifecycle === "validating" ||
      candidate.reconnectLifecycle === "spawning" ||
      candidate.reconnectLifecycle === "binding";
    if (isLive || isReconnecting) return candidate;
  }
  return null;
}

export function hasDuplicateActiveProviderClaim(
  session: TerminalSession,
  allSessions?: Readonly<Record<string, TerminalSession>> | readonly TerminalSession[],
): boolean {
  return findConflictingActiveSession(session, allSessions) !== null;
}

export function getAgentReconnectAffordance(
  session: TerminalSession,
  allSessions?: Readonly<Record<string, TerminalSession>> | readonly TerminalSession[],
): AgentReconnectAffordance {
  if (typeof session.agentType !== "string" || session.agentType.trim() === "") {
    return affordance("none", session, { reason: "Not an agent session", agentType: null, canReconnect: false });
  }

  const agentType = session.agentType.trim();
  if (!canResumeAgent(agentType)) {
    return affordance("unsupported", session, {
      canReconnect: false,
      reason: `Agent "${agentType}" does not support session resume`,
    });
  }
  if (!canCaptureAuthoritativeProviderSession(agentType)) {
    return affordance("unsupported", session, {
      canReconnect: false,
      reason: `Agent "${agentType}" does not have an authoritative session capture integration`,
    });
  }

  const providerSession = extractNormalizedProviderSession(session);
  if (!providerSession) {
    return affordance("missing_reference", session, {
      canReconnect: false,
      reason: `Missing session reference for agent "${agentType}"`,
    });
  }

  const argv = buildResumeArgv({ agentType, providerSession });
  if (!argv) {
    return affordance("missing_reference", session, {
      canReconnect: false,
      providerSession,
      reason: `Invalid resume configuration for agent "${agentType}"`,
    });
  }

  if (session.lifecycle !== "exited" || session.backendSessionId !== null) {
    return affordance("none", session, {
      canReconnect: false,
      providerSession,
      reconnectLifecycle: "idle",
      reason: "Session is already active",
      argv,
    });
  }

  const conflicting = findConflictingActiveSession(session, allSessions);
  if (conflicting) {
    return affordance("conflict", session, {
      canReconnect: false,
      providerSession,
      reason: `Agent session "${providerSession.id}" is already active in another pane (${conflicting.id}).`,
      conflictingSessionId: conflicting.id,
    });
  }

  const reconnectLifecycle: ReconnectLifecycle = session.reconnectLifecycle ?? "idle";

  if (reconnectLifecycle === "validating" || reconnectLifecycle === "spawning" || reconnectLifecycle === "binding") {
    return affordance("reconnecting", session, {
      canReconnect: false,
      isReconnecting: true,
      providerSession,
      reconnectLifecycle,
      argv,
    });
  }

  if (reconnectLifecycle === "failed") {
    return affordance("failed", session, {
      canReconnect: true,
      canRetry: true,
      providerSession,
      reconnectLifecycle: "failed",
      error: session.reconnectError ?? null,
      reason: session.reconnectError?.message ?? "Previous reconnect attempt failed",
      argv,
    });
  }

  return affordance("idle", session, {
    canReconnect: true,
    providerSession,
    reconnectLifecycle: "idle",
    argv,
  });
}

export function resumableAgentPane(
  session: TerminalSession,
  allSessions?: Readonly<Record<string, TerminalSession>> | readonly TerminalSession[],
): ResumableAgentPane | null {
  const afford = getAgentReconnectAffordance(session, allSessions);
  if (
    (afford.status !== "idle" && afford.status !== "failed") ||
    !afford.argv ||
    !afford.providerSession ||
    !afford.agentType
  ) {
    return null;
  }

  return {
    sessionId: session.id,
    agentType: afford.agentType,
    agentSessionId: afford.providerSession.id,
    providerSession: afford.providerSession,
    cwd: session.cwd,
    argv: afford.argv,
  };
}

export function collectResumableAgentPanes(
  sessions: Readonly<Record<string, TerminalSession>>,
): ResumableAgentPane[] {
  const result: ResumableAgentPane[] = [];
  for (const session of Object.values(sessions)) {
    const pane = resumableAgentPane(session, sessions);
    if (pane !== null) {
      result.push(pane);
    }
  }
  return result;
}
