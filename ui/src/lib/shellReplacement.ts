import { closeTerminal, listTerminalSessions, spawnTerminalDetailed, toIpcError } from "./tauri";
import { isStandbyBackendSessionId } from "./sessionLifecycle";
import { safeRandomUUID } from "./uuid";
import type { SpawnTerminalRequest, SpawnTerminalResult } from "./tauri";
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

/**
 * A pane restart replaces the process behind an already-live pane without changing its identity.
 * Unlike `replaceExitedShellSession` it accepts a running session: the caller has already decided
 * that the pane's process should start over (Ctrl+Shift+R or the pane's restart affordance).
 */
export type RestartPaneOptions = ShellReplacementOptions & {
  cols?: number | null;
  rows?: number | null;
  /** Force the new process to start in this directory instead of the session's live CWD. */
  cwd?: string | null;
  /** Startup command for the replacement, e.g. an agent continuation chosen by the user. */
  startup?: SpawnTerminalRequest["startup"];
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

/**
 * Restart the process behind a live pane, keeping the pane, its tab and its local session id.
 *
 * The old backend is closed only after the replacement exists, so a failed spawn leaves the pane
 * with its original process (the caller keeps the pane mounted and offers a retry). Agent panes
 * need an explicit choice: `clearAgent` starts a plain shell, while the default keeps the detected
 * agent type so the resumed agent can continue the same provider session.
 *
 * The one exception is continuing the pane's own conversation (`startup.kind === "agentResume"`):
 * the daemon admits that spawn only once the outgoing process no longer claims the provider
 * session, so the old backend is retired first. A failure after that point leaves the pane on its
 * inline restart error with Retry, and the retry spawns against a freed claim.
 */
export function restartPaneSession(
  localSessionId: string,
  dependencies: ShellReplacementDependencies,
  options?: RestartPaneOptions,
): Promise<SpawnTerminalResult> {
  const existing = inFlightReplacements.get(localSessionId);
  if (existing) return existing;

  const attempt = (async () => {
    const initial = dependencies.getSessions()[localSessionId];
    if (!initial) throw invalidReplacement("Terminal session cannot be restarted");
    // A paired pane restarts through its host's daemon, exactly like an exited paired shell does.
    const pairedStartup = isPairedWorkspaceId(initial.workspaceId)
      ? resolvePairedStartup(initial.workspaceId)
      : null;
    if (isPairedWorkspaceId(initial.workspaceId) && !pairedStartup) {
      throw invalidReplacement("Paired workspace is not registered for this machine");
    }

    const previousBackendSessionId = initial.backendSessionId;
    let spawned: SpawnTerminalResult | null = null;
    try {
      const clearAgent = options?.clearAgent === true;
      // Continuing this pane's own agent conversation is a take-over, not a second owner: the
      // daemon refuses a spawn whose provider session is still claimed, and that claim belongs to
      // the process being replaced. Retire the old process first so the replacement can adopt it.
      // A backend that is already gone fails here harmlessly - the daemon prunes dead ownership
      // when it admits the spawn.
      const continuesOwnConversation = options?.startup?.kind === "agentResume";
      if (continuesOwnConversation && previousBackendSessionId) {
        await (dependencies.close ?? closeTerminal)(previousBackendSessionId).catch(() => undefined);
      }
      spawned = await (dependencies.spawn ?? spawnTerminalDetailed)({
        workspaceId: initial.workspaceId,
        worktree: initial.worktree,
        cwd: options?.cwd ?? initial.cwd,
        cols: options?.cols ?? null,
        rows: options?.rows ?? null,
        clientRequestId: (dependencies.createRequestId ?? (() => `pane-restart-${safeRandomUUID()}`))(),
        startup: options?.startup ?? pairedStartup,
      });
      const requireCurrent = (): TerminalSession => {
        const current = dependencies.getSessions()[localSessionId];
        if (!current || current.workspaceId !== initial.workspaceId) {
          throw invalidReplacement("Terminal session changed while restarting the pane");
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
        cwd: spawned.session.cwd ?? options?.cwd ?? current.cwd,
        daemonEpoch: spawned.daemonEpoch,
        ...(spawned.session.incarnation != null ? { incarnation: spawned.session.incarnation } : {}),
        clearAgent,
      });
    } catch (error) {
      // Retire only a replacement that never took over. After a resume the original process is
      // already gone, so the pane stays on its inline restart error and Retry spawns a fresh one.
      if (spawned) await (dependencies.close ?? closeTerminal)(spawned.sessionId).catch(() => undefined);
      throw toIpcError(error);
    }

    // The replacement owns the pane now; retire the old process only after the rebind landed.
    // A resume restart already retired it, because the conversation could not have two owners.
    if (previousBackendSessionId && options?.startup?.kind !== "agentResume") {
      await (dependencies.close ?? closeTerminal)(previousBackendSessionId).catch(() => undefined);
    }
    return spawned;
  })();

  inFlightReplacements.set(localSessionId, attempt);
  void attempt.finally(() => {
    if (inFlightReplacements.get(localSessionId) === attempt) inFlightReplacements.delete(localSessionId);
  }).catch(() => undefined);
  return attempt;
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
