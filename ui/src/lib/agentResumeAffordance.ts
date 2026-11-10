import {
  agentProviderSessionsEqual,
  buildAgentContinueStartup,
  buildResumeArgv,
  canCaptureAuthoritativeProviderSession,
  canResumeAgent,
  normalizeAgentProviderSession,
  verifiedContinuationModes,
} from "./agentResume";
import { isPairedWorkspaceId, isRemoteWorkspaceId } from "./remoteProject";
import { isStandbyBackendSessionId } from "./sessionLifecycle";
import type { AgentContinueMode, AgentContinueStartup } from "./tauri";
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
    return normalizeAgentProviderSession({ key: "session_id", id: session.agentSessionId });
  }
  return null;
}

function hasLiveBackend(session: TerminalSession): boolean {
  return session.lifecycle !== "exited" && session.backendSessionId !== null && !isStandbyBackendSessionId(session.backendSessionId);
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

    const isLive = hasLiveBackend(candidate) || candidate.lifecycle !== "exited";
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

  if (session.lifecycle !== "exited" || hasLiveBackend(session)) {
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

/** Why a continuation mode is hidden. There is no stop reason: continuation never stops the source. */
export const CONTINUATION_UNAVAILABLE_REASONS = [
  "sourceWorking",
  "liveClaim",
  "providerUnsupported",
  "missingNativeHistory",
  "remoteCapabilityAbsent",
] as const;

export type ContinuationUnavailableReason = (typeof CONTINUATION_UNAVAILABLE_REASONS)[number];

export type ContinuationModeOffer = {
  readonly mode: AgentContinueMode;
  readonly available: boolean;
  readonly reason: ContinuationUnavailableReason | null;
};

export type ContinuationAffordance = {
  readonly availableModes: readonly AgentContinueMode[];
  readonly modes: readonly ContinuationModeOffer[];
  /** Startup for an available mode, otherwise null. Never includes argv and never stops the source. */
  readonly startup: (mode: AgentContinueMode) => AgentContinueStartup | null;
};

export type ContinuationAffordanceContext = {
  readonly sessions?: Readonly<Record<string, TerminalSession>> | readonly TerminalSession[];
  /** `false` means the native transcript or provenance was not verified. Omitted does not invent absence. */
  readonly nativeHistory?: boolean;
  /** Required advertisement for a remote or paired owning host. Local omission means the desktop daemon. */
  readonly terminalContinueV1?: boolean;
  readonly remoteOwner?: boolean;
  /** Agent activity, when the session lifecycle has not yet caught up. */
  readonly activityState?: "working" | "waiting" | "done";
};

function continuationPanes(
  sessions: ContinuationAffordanceContext["sessions"],
): readonly TerminalSession[] {
  if (!sessions) return [];
  return Array.isArray(sessions) ? sessions : Object.values(sessions);
}

function sourceIsWorking(session: TerminalSession, context: ContinuationAffordanceContext): boolean {
  return session.lifecycle === "working" || session.lifecycle === "starting" || context.activityState === "working";
}

function sourceIsReconnecting(session: TerminalSession): boolean {
  return (
    session.reconnectLifecycle === "validating"
    || session.reconnectLifecycle === "spawning"
    || session.reconnectLifecycle === "binding"
  );
}

/** Exited source with no live backend and no reconnect in flight. Desktop publication uses this same predicate. */
export function isExitedContinuationSource(session: TerminalSession): boolean {
  return session.lifecycle === "exited" && !hasLiveBackend(session) && !sourceIsReconnecting(session);
}

function sourceIsExited(session: TerminalSession): boolean {
  return isExitedContinuationSource(session);
}

function sameProviderClaim(source: TerminalSession, candidate: TerminalSession): boolean {
  if (candidate.id === source.id || !source.agentType || !candidate.agentType) return false;
  if (candidate.agentType.trim().toLowerCase() !== source.agentType.trim().toLowerCase()) return false;
  const left = extractNormalizedProviderSession(source);
  const right = extractNormalizedProviderSession(candidate);
  return Boolean(left && right && agentProviderSessionsEqual(source.agentType, left, right));
}

function otherWorkingClaim(source: TerminalSession, sessions: readonly TerminalSession[]): boolean {
  return sessions.some((candidate) =>
    sameProviderClaim(source, candidate)
    && (candidate.lifecycle === "working" || candidate.lifecycle === "starting"),
  );
}

function otherLiveClaim(source: TerminalSession, sessions: readonly TerminalSession[]): boolean {
  return sessions.some((candidate) => {
    if (!sameProviderClaim(source, candidate)) return false;
    const live = hasLiveBackend(candidate) || candidate.lifecycle !== "exited";
    const reconnecting =
      candidate.reconnectLifecycle === "validating"
      || candidate.reconnectLifecycle === "spawning"
      || candidate.reconnectLifecycle === "binding";
    return live || reconnecting;
  });
}

function remoteCapabilityMissing(session: TerminalSession, context: ContinuationAffordanceContext): boolean {
  const remote = context.remoteOwner === true
    || (context.remoteOwner !== false && (isRemoteWorkspaceId(session.workspaceId) || isPairedWorkspaceId(session.workspaceId)));
  return remote && context.terminalContinueV1 !== true;
}

function continuationReason(
  session: TerminalSession,
  mode: AgentContinueMode,
  context: ContinuationAffordanceContext,
): ContinuationUnavailableReason | null {
  if (remoteCapabilityMissing(session, context)) return "remoteCapabilityAbsent";
  if (!verifiedContinuationModes(session.agentType ?? "").includes(mode)) return "providerUnsupported";
  if (context.nativeHistory === false) return "missingNativeHistory";
  const providerSession = extractNormalizedProviderSession(session);
  if (!providerSession || !session.agentType) return "missingNativeHistory";
  if (!buildAgentContinueStartup(session.agentType, providerSession, mode)) return "missingNativeHistory";
  if (sourceIsWorking(session, context)) return "sourceWorking";
  const others = continuationPanes(context.sessions);
  if (mode === "resume" && (!sourceIsExited(session) || otherLiveClaim(session, others))) return "liveClaim";
  if (mode === "fork" && otherWorkingClaim(session, others)) return "liveClaim";
  return null;
}

/**
 * Continuation affordance for one pane. Resume needs an exited source and no live claim.
 * Fork needs a non-working source. Neither path stops the source.
 */
export function getContinuationAffordance(
  session: TerminalSession,
  context: ContinuationAffordanceContext = {},
): ContinuationAffordance {
  const modes = (["fork", "resume"] as const).map((mode): ContinuationModeOffer => {
    const reason = continuationReason(session, mode, context);
    return { mode, available: reason === null, reason };
  });
  return {
    modes,
    availableModes: modes.filter((mode) => mode.available).map((mode) => mode.mode),
    startup(mode) {
      const offer = modes.find((candidate) => candidate.mode === mode);
      if (!offer?.available || !session.agentType) return null;
      const providerSession = extractNormalizedProviderSession(session);
      if (!providerSession) return null;
      return buildAgentContinueStartup(session.agentType, providerSession, mode);
    },
  };
}

/** Startup DTO for replacing the process behind the *same* pane with its own conversation. */
export type InPlaceRestartStartup = {
  readonly kind: "agentResume";
  readonly agentType: string;
  readonly providerSession: AgentProviderSession;
};

/**
 * Restarting a pane in place keeps its local session, so the replacement may take over the
 * provider session that pane already holds. Unlike `getContinuationAffordance` - which refuses a
 * live source because a *second* pane would be a real conflict - this never consults exit state:
 * the source process is the one being replaced. The daemon admits it once that owner is closed.
 */
export function getInPlaceRestartStartup(session: TerminalSession): InPlaceRestartStartup | null {
  const agentType = session.agentType?.trim();
  if (!agentType || !canResumeAgent(agentType)) return null;
  // A remote or paired pane is restarted by its owning host; this desktop daemon must not claim
  // a conversation that lives on another machine.
  if (isRemoteWorkspaceId(session.workspaceId) || isPairedWorkspaceId(session.workspaceId)) return null;
  const providerSession = extractNormalizedProviderSession(session);
  if (!providerSession) return null;
  if (!buildResumeArgv({ agentType, providerSession })) return null;
  return { kind: "agentResume", agentType: agentType.toLowerCase(), providerSession };
}
