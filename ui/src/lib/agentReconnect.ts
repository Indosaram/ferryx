import { getAgentReconnectAffordance } from "./agentResumeAffordance";
import { closeTerminal, spawnTerminalDetailed, toIpcError } from "./tauri";
import type { SpawnTerminalResult } from "./tauri";
import type { StructuredIpcError, TerminalSession } from "./types";

type ReconnectAction =
  | { type: "SET_RECONNECT_LIFECYCLE"; sessionId: string; lifecycle: "validating" | "spawning" | "binding" | "failed"; error?: StructuredIpcError | null; requestId?: string | null }
  | { type: "REBIND_SESSION_BACKEND"; sessionId: string; backendSessionId: string; cwd?: string; daemonEpoch?: string | null };

export type AgentReconnectDependencies = {
  getSessions: () => Readonly<Record<string, TerminalSession>>;
  dispatch: (action: ReconnectAction) => void;
  spawn?: typeof spawnTerminalDetailed;
  attach: (result: SpawnTerminalResult, localSession: TerminalSession) => Promise<void>;
  close?: (backendSessionId: string) => Promise<void>;
  persist?: (result: SpawnTerminalResult, localSession: TerminalSession) => void | Promise<void>;
  createRequestId?: () => string;
};

const inFlightReconnects = new Map<string, Promise<SpawnTerminalResult>>();

function defaultRequestId(): string {
  return `agent-reconnect-${crypto.randomUUID()}`;
}

function invalidReconnect(message: string): StructuredIpcError {
  return { code: "AGENT_RESUME_INVALID", message };
}

export function reconnectAgentSession(
  localSessionId: string,
  dependencies: AgentReconnectDependencies,
): Promise<SpawnTerminalResult> {
  const existing = inFlightReconnects.get(localSessionId);
  if (existing) return existing;

  const attempt = (async () => {
    const initial = dependencies.getSessions()[localSessionId];
    let spawned: SpawnTerminalResult | null = null;
    try {
      if (!initial) throw invalidReconnect("Terminal session no longer exists");
      dependencies.dispatch({ type: "SET_RECONNECT_LIFECYCLE", sessionId: localSessionId, lifecycle: "validating" });
      const affordance = getAgentReconnectAffordance(initial, dependencies.getSessions());
      if (!affordance.canReconnect || !affordance.agentType || !affordance.providerSession) {
        throw invalidReconnect(affordance.reason ?? "Agent session cannot reconnect");
      }
      const providerSession = affordance.providerSession;

      const requestId = initial.reconnectRequestId ?? (dependencies.createRequestId ?? defaultRequestId)();
      dependencies.dispatch({ type: "SET_RECONNECT_LIFECYCLE", sessionId: localSessionId, lifecycle: "spawning", requestId });
      const spawn = dependencies.spawn ?? spawnTerminalDetailed;
      spawned = await spawn({
        workspaceId: initial.workspaceId,
        worktree: initial.worktree,
        cwd: initial.cwd,
        clientRequestId: requestId,
        startup: {
          kind: "agentResume",
          agentType: affordance.agentType,
          providerSession,
        },
      });
      const requireCurrentIdentity = (): TerminalSession => {
        const current = dependencies.getSessions()[localSessionId];
        if (
          !current
          || current.workspaceId !== initial.workspaceId
          || current.providerSession?.key !== providerSession.key
          || current.providerSession.id !== providerSession.id
        ) {
          throw invalidReconnect("Terminal session changed while reconnecting");
        }
        return current;
      };
      let current = requireCurrentIdentity();
      dependencies.dispatch({ type: "SET_RECONNECT_LIFECYCLE", sessionId: localSessionId, lifecycle: "binding" });
      await dependencies.attach(spawned, current);
      current = requireCurrentIdentity();
      await dependencies.persist?.(spawned, current);
      current = requireCurrentIdentity();
      dependencies.dispatch({
        type: "REBIND_SESSION_BACKEND",
        sessionId: localSessionId,
        backendSessionId: spawned.sessionId,
        cwd: spawned.session.cwd ?? current.cwd,
        daemonEpoch: spawned.daemonEpoch,
      });
      return spawned;
    } catch (error) {
      if (spawned) {
        await (dependencies.close ?? closeTerminal)(spawned.sessionId).catch(() => undefined);
      }
      const structured = toIpcError(error);
      dependencies.dispatch({
        type: "SET_RECONNECT_LIFECYCLE",
        sessionId: localSessionId,
        lifecycle: "failed",
        error: structured,
      });
      throw structured;
    }
  })();

  inFlightReconnects.set(localSessionId, attempt);
  void attempt.finally(() => {
    if (inFlightReconnects.get(localSessionId) === attempt) inFlightReconnects.delete(localSessionId);
  }).catch(() => undefined);
  return attempt;
}

export function clearAgentReconnectInflightForTests(): void {
  inFlightReconnects.clear();
}
