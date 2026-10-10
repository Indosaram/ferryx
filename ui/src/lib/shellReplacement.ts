import { closeTerminal, listTerminalSessions, spawnTerminalDetailed, toIpcError } from "./tauri";
import { isStandbyBackendSessionId } from "./sessionLifecycle";
import { safeRandomUUID } from "./uuid";
import type { SpawnTerminalResult } from "./tauri";
import type { StructuredIpcError, TerminalSession } from "./types";
import { isPairedWorkspaceId, isRemoteWorkspaceId, resolvePairedStartup } from "./remoteProject";

type RebindAction = {
  type: "REBIND_SESSION_BACKEND";
  sessionId: string;
  backendSessionId: string;
  cwd?: string;
  daemonEpoch?: string | null;
  incarnation?: string | null;
  clearAgent?: boolean;
};

export type ShellReplacementDependencies = {
  getSessions: () => Readonly<Record<string, TerminalSession>>;
  dispatch: (action: RebindAction) => void;
  spawn?: typeof spawnTerminalDetailed;
  close?: (backendSessionId: string) => Promise<void>;
  persist?: (result: SpawnTerminalResult, localSession: TerminalSession) => void | Promise<void>;
  createRequestId?: () => string;
};

const inFlightReplacements = new Map<string, Promise<SpawnTerminalResult>>();

function invalidReplacement(message: string): StructuredIpcError {
  return { code: "AGENT_RESUME_INVALID", message };
}

function hasReplaceableBackend(session: TerminalSession): boolean {
  return (
    session.backendSessionId === null ||
    isStandbyBackendSessionId(session.backendSessionId) ||
    session.lifecycle === "exited" ||
    session.lifecycle === "failed"
  );
}

export type ShellReplacementOptions = {
  clearAgent?: boolean;
};

export function replaceExitedShellSession(
  localSessionId: string,
  dependencies: ShellReplacementDependencies,
  options?: ShellReplacementOptions,
): Promise<SpawnTerminalResult> {
  const existing = inFlightReplacements.get(localSessionId);
  if (existing) return existing;

  const attempt = (async () => {
    const initial = dependencies.getSessions()[localSessionId];
    let spawned: SpawnTerminalResult | null = null;
    try {
      const shouldBlockAgent = Boolean(initial?.agentType && !options?.clearAgent);
      const isPaired = isPairedWorkspaceId(initial?.workspaceId);
      const pairedStartup = isPaired && initial ? resolvePairedStartup(initial.workspaceId) : null;
      if (!initial || !hasReplaceableBackend(initial) || shouldBlockAgent || isRemoteWorkspaceId(initial.workspaceId) || (isPaired && !pairedStartup)) {
        throw invalidReplacement("Terminal session cannot be replaced with a new shell");
      }
      spawned = await (dependencies.spawn ?? spawnTerminalDetailed)({
        workspaceId: initial.workspaceId,
        worktree: initial.worktree,
        cwd: initial.cwd,
        clientRequestId: (dependencies.createRequestId ?? (() => `shell-replacement-${safeRandomUUID()}`))(),
        startup: pairedStartup,
      });
      const requireCurrent = (): TerminalSession => {
        const current = dependencies.getSessions()[localSessionId];
        const shouldBlockCurrentAgent = Boolean(current?.agentType && !options?.clearAgent);
        if (!current || !hasReplaceableBackend(current) || shouldBlockCurrentAgent || isRemoteWorkspaceId(current.workspaceId) || current.workspaceId !== initial.workspaceId) {
          throw invalidReplacement("Terminal session changed while opening a new shell");
        }
        return current;
      };
      let current = requireCurrent();
      await dependencies.persist?.(spawned, current);
      current = requireCurrent();
      dependencies.dispatch({
        type: "REBIND_SESSION_BACKEND",
        sessionId: localSessionId,
        backendSessionId: spawned.sessionId,
        cwd: spawned.session.cwd ?? current.cwd,
        daemonEpoch: spawned.daemonEpoch,
        ...(spawned.session.incarnation != null ? { incarnation: spawned.session.incarnation } : {}),
        clearAgent: options?.clearAgent,
      });
      return spawned;
    } catch (error) {
      if (spawned) await (dependencies.close ?? closeTerminal)(spawned.sessionId).catch(() => undefined);
      throw toIpcError(error);
    }
  })();

  inFlightReplacements.set(localSessionId, attempt);
  void attempt.finally(() => {
    if (inFlightReplacements.get(localSessionId) === attempt) inFlightReplacements.delete(localSessionId);
  }).catch(() => undefined);
  return attempt;
}

export function clearShellReplacementInflightForTests(): void {
  inFlightReplacements.clear();
}

export type ReconnectLocalSessionDependencies = {
  getSessions: () => Readonly<Record<string, TerminalSession>>;
  list?: () => Promise<Array<{ sessionId: string; daemonEpoch?: string | null; incarnation?: string | null; worktreePath?: string | null; running?: boolean }>>;
  dispatch: (action: RebindAction) => void;
  persist?: (session: TerminalSession) => void | Promise<void>;
  openNewShell: (sessionId: string) => Promise<void> | void;
};

export async function reconnectLocalSession(
  sessionId: string,
  dependencies: ReconnectLocalSessionDependencies,
): Promise<void> {
  const session = dependencies.getSessions()[sessionId];
  if (!session) return;

  if (session.backendSessionId) {
    try {
      const liveSummaries = await (dependencies.list ?? listTerminalSessions)();
      const live = liveSummaries.find(
        (candidate) => candidate.sessionId === session.backendSessionId && candidate.running !== false,
      );
      if (live) {
        const daemonEpoch = live.daemonEpoch ?? session.daemonEpoch;
        const incarnation = live.incarnation ?? session.incarnation ?? null;
        const updatedSession: TerminalSession = {
          ...session,
          daemonEpoch,
          ...(incarnation != null ? { incarnation } : {}),
        };
        await dependencies.persist?.(updatedSession);
        dependencies.dispatch({
          type: "REBIND_SESSION_BACKEND",
          sessionId,
          backendSessionId: session.backendSessionId,
          cwd: session.cwd,
          daemonEpoch,
          ...(incarnation != null ? { incarnation } : {}),
        });
        return;
      }
    } catch {
      // Backend session dead or daemon unreachable; fall through to openNewShell
    }
  }

  await dependencies.openNewShell(sessionId);
}
