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
    session.lifecycle === "failed" ||
    // A local pane stuck reconnecting (e.g. its PTY died with an OS reboot) offers "Open new shell"
    // as its escape; the retained backend id must not make that escape fail. The old PTY is left
    // untouched: if recovery wins the race, `requireCurrent` sees it rebound and discards the spawn.
    (session.remoteConnectionState === "reconnecting" &&
      !isRemoteWorkspaceId(session.workspaceId) &&
      !isPairedWorkspaceId(session.workspaceId))
  );
}

export type ShellReplacementOptions = {
  clearAgent?: boolean;
};

function hasConfirmedGoneBackend(session: TerminalSession): boolean {
  return isStandbyBackendSessionId(session.backendSessionId);
}

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
      // An SSH pane is replaceable only once its backend is confirmed gone (standby/null): a live
      // or merely disconnected SSH target is reattached, never replaced. The daemon resolves the
      // SSH workspace to its remote host, so the new shell starts there, never locally.
      const isSsh = isRemoteWorkspaceId(initial?.workspaceId);
      if (!initial || !hasReplaceableBackend(initial) || shouldBlockAgent || (isSsh && !hasConfirmedGoneBackend(initial)) || (isPaired && !pairedStartup)) {
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
        if (!current || !hasReplaceableBackend(current) || shouldBlockCurrentAgent || (isSsh && !hasConfirmedGoneBackend(current)) || current.workspaceId !== initial.workspaceId) {
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
  openNewShell?: (sessionId: string) => Promise<void> | void;
  recover?: (sessionId: string) => Promise<void> | void;
};

export async function reconnectLocalSession(
  sessionId: string,
  dependencies: ReconnectLocalSessionDependencies,
): Promise<void> {
  const session = dependencies.getSessions()[sessionId];
  if (!session) return;

  // Share retained recovery owner with automatic paths when recover dependency is provided
  if (dependencies.recover) {
    await dependencies.recover(sessionId);
    return;
  }

  if (session.backendSessionId) {
    const fresh = () => {
      const current = dependencies.getSessions()[sessionId];
      return Boolean(current && current.backendSessionId === session.backendSessionId &&
        current.incarnation === session.incarnation && current.daemonEpoch === session.daemonEpoch &&
        current.recoveryKey === session.recoveryKey && current.recoveryToken === session.recoveryToken);
    };
    try {
      const liveSummaries = await (dependencies.list ?? listTerminalSessions)();

      // Post-await fence against state change during query
      const currentPostQuery = dependencies.getSessions()[sessionId];
      if (!currentPostQuery || !fresh()) {
        return;
      }

      const live = liveSummaries.find(
        (candidate) => candidate.sessionId === session.backendSessionId && candidate.running !== false,
      );
      if (live) {
        // Reject an observed different incarnation rather than adopt it (identity conflict)
        const liveIncarnation = live.incarnation ?? null;
        const currentIncarnation = currentPostQuery.incarnation ?? null;
        if (!currentIncarnation || !liveIncarnation || currentIncarnation !== liveIncarnation || !live.daemonEpoch) {
          return;
        }

        const daemonEpoch = live.daemonEpoch ?? currentPostQuery.daemonEpoch;
        const incarnation = liveIncarnation ?? currentIncarnation;
        const updatedSession: TerminalSession = {
          ...currentPostQuery,
          lifecycle: "running",
          remoteConnectionState: undefined,
          daemonEpoch,
          ...(incarnation != null ? { incarnation } : {}),
        };

        // Fence before and after persist
        if (!fresh()) return;
        if (dependencies.persist) {
          await dependencies.persist(updatedSession);
        }
        const currentPostPersist = dependencies.getSessions()[sessionId];
        if (!currentPostPersist || !fresh()) {
          return;
        }

        dependencies.dispatch({
          type: "REBIND_SESSION_BACKEND",
          sessionId,
          backendSessionId: session.backendSessionId,
          cwd: currentPostPersist.cwd,
          daemonEpoch,
          ...(incarnation != null ? { incarnation } : {}),
        });
        return;
      }
      // Scoped absence / unknown liveness does NOT permit spawning a replacement shell (INV-1 / Task 3)
      return;
    } catch {
      // Query failure / daemon unreachable is transient uncertainty, NOT permission to spawn a shell
      return;
    }
  }

  // Distinguish historically retained null IDs from genuinely never-created sessions
  const hasHistoricalRecoveryProvenance = Boolean(
    session.recoveryKey != null ||
    session.remoteConnectionState === "reconnecting" ||
    session.reconnectLifecycle != null ||
    session.recoveryToken != null ||
    session.backendUnavailableReason != null
  );
  if (!session.backendSessionId && !hasHistoricalRecoveryProvenance && dependencies.openNewShell) {
    await dependencies.openNewShell(sessionId);
  }
}
