import { isPairedWorkspaceId } from "./remoteProject";
import { getAgentReconnectAffordance } from "./agentResumeAffordance";
import { safeRandomUUID } from "./uuid";
import { switchDebug } from "./switchDebug";
import { closeTerminal, spawnTerminalDetailed, toIpcError } from "./tauri";
import type { SpawnTerminalResult } from "./tauri";
import type { StructuredIpcError, TerminalSession } from "./types";

type ReconnectAction =
  | { type: "SET_RECONNECT_LIFECYCLE"; sessionId: string; lifecycle: "validating" | "spawning" | "binding" | "failed"; error?: StructuredIpcError | null; requestId?: string | null }
  | { type: "REBIND_SESSION_BACKEND"; sessionId: string; backendSessionId: string; cwd?: string; daemonEpoch?: string | null; incarnation?: string | null };

export type AgentReconnectDependencies = {
  getSessions: () => Readonly<Record<string, TerminalSession>>;
  dispatch: (action: ReconnectAction) => void;
  spawn?: typeof spawnTerminalDetailed;
  attach: (result: SpawnTerminalResult, localSession: TerminalSession) => Promise<void>;
  close?: (backendSessionId: string) => Promise<void>;
  persist?: (result: SpawnTerminalResult, localSession: TerminalSession) => void | Promise<void>;
  createRequestId?: () => string;
  /** Overall budget for spawn + attach + persist; defaults to AGENT_RECONNECT_DEADLINE_MS. */
  deadlineMs?: number;
};

/** Overall deadline for one reconnect attempt so a hung stage lands on Retry instead of spinning forever. */
export const AGENT_RECONNECT_DEADLINE_MS = 30_000;

type ReconnectStage = "spawn" | "attach" | "persist";

const inFlightReconnects = new Map<string, Promise<SpawnTerminalResult>>();

function defaultRequestId(): string {
  return `agent-reconnect-${safeRandomUUID()}`;
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
    const startedAt = Date.now();
    const deadlineMs = dependencies.deadlineMs ?? AGENT_RECONNECT_DEADLINE_MS;
    const close = dependencies.close ?? closeTerminal;
    // Race one stage against the remaining attempt budget; the timer is always cleared.
    const runStage = async <T>(stage: ReconnectStage, work: Promise<T>, onTimeout?: () => void): Promise<T> => {
      const log = (phase: "begin" | "end" | "timeout" | "failure", errorCode: string | null) => switchDebug("agent.reconnect.stage", {
        sessionId: localSessionId, stage, phase, elapsedMs: Date.now() - startedAt, errorCode,
      });
      const timeoutError: StructuredIpcError = {
        code: "SPAWN_ATTEMPT_TIMEOUT",
        message: "Agent reconnect did not finish in time. Retry to reconnect.",
        details: { stage, delivery: spawned ? "confirmed" : "ambiguous" },
      };
      let timer: ReturnType<typeof setTimeout> | undefined;
      log("begin", null);
      try {
        const value = await Promise.race([
          work,
          new Promise<never>((_, reject) => {
            timer = setTimeout(() => reject(timeoutError), Math.max(0, deadlineMs - (Date.now() - startedAt)));
          }),
        ]);
        log("end", null);
        return value;
      } catch (error) {
        if (error === timeoutError) {
          onTimeout?.();
          log("timeout", timeoutError.code);
        } else {
          log("failure", toIpcError(error).code);
        }
        throw error;
      } finally {
        clearTimeout(timer);
      }
    };
    try {
      if (!initial) throw invalidReconnect("Terminal session no longer exists");
      if (isPairedWorkspaceId(initial.workspaceId)) throw {
        code: "UNSUPPORTED_CAPABILITY", message: "Paired terminal recovery requires native remote terminal support. No local agent was started.",
      };
      dependencies.dispatch({ type: "SET_RECONNECT_LIFECYCLE", sessionId: localSessionId, lifecycle: "validating" });
      const affordance = getAgentReconnectAffordance(initial, dependencies.getSessions());
      if (!affordance.canReconnect || !affordance.agentType || !affordance.providerSession) {
        throw invalidReconnect(affordance.reason ?? "Agent session cannot reconnect");
      }
      const providerSession = affordance.providerSession;

      const requestId = initial.reconnectRequestId ?? (dependencies.createRequestId ?? defaultRequestId)();
      dependencies.dispatch({ type: "SET_RECONNECT_LIFECYCLE", sessionId: localSessionId, lifecycle: "spawning", requestId });
      const spawn = dependencies.spawn ?? spawnTerminalDetailed;
      const rawSpawn = Promise.resolve(spawn({
        workspaceId: initial.workspaceId,
        worktree: initial.worktree,
        cwd: initial.cwd,
        clientRequestId: requestId,
        startup: {
          kind: "agentResume",
          agentType: affordance.agentType,
          providerSession,
        },
      }));
      let spawnAbandoned = false;
      // A spawn that settles after the deadline has no owner: close it and never rebind.
      void rawSpawn.then((late) => {
        if (spawnAbandoned) void close(late.sessionId).catch(() => undefined);
      }, () => undefined);
      spawned = await runStage("spawn", rawSpawn, () => { spawnAbandoned = true; });
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
      await runStage("attach", dependencies.attach(spawned, current));
      current = requireCurrentIdentity();
      if (dependencies.persist) await runStage("persist", Promise.resolve(dependencies.persist(spawned, current)));
      current = requireCurrentIdentity();
      dependencies.dispatch({
        type: "REBIND_SESSION_BACKEND",
        sessionId: localSessionId,
        backendSessionId: spawned.sessionId,
        cwd: spawned.session.cwd ?? current.cwd,
        daemonEpoch: spawned.daemonEpoch,
        incarnation: spawned.session.incarnation ?? null,
      });
      return spawned;
    } catch (error) {
      if (spawned) {
        await close(spawned.sessionId).catch(() => undefined);
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
