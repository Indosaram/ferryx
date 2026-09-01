import { closeTerminal, spawnTerminalDetailed, toIpcError } from "./tauri";
import type { SpawnTerminalResult } from "./tauri";
import type { StructuredIpcError, TerminalSession } from "./types";

type RebindAction = {
  type: "REBIND_SESSION_BACKEND";
  sessionId: string;
  backendSessionId: string;
  cwd?: string;
  daemonEpoch?: string | null;
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

export function replaceExitedShellSession(
  localSessionId: string,
  dependencies: ShellReplacementDependencies,
): Promise<SpawnTerminalResult> {
  const existing = inFlightReplacements.get(localSessionId);
  if (existing) return existing;

  const attempt = (async () => {
    const initial = dependencies.getSessions()[localSessionId];
    let spawned: SpawnTerminalResult | null = null;
    try {
      if (!initial || initial.backendSessionId !== null || initial.agentType) {
        throw invalidReplacement("Terminal session cannot be replaced with a new shell");
      }
      spawned = await (dependencies.spawn ?? spawnTerminalDetailed)({
        workspaceId: initial.workspaceId,
        worktree: initial.worktree,
        cwd: initial.cwd,
        clientRequestId: (dependencies.createRequestId ?? (() => `shell-replacement-${crypto.randomUUID()}`))(),
        startup: null,
      });
      const requireCurrent = (): TerminalSession => {
        const current = dependencies.getSessions()[localSessionId];
        if (!current || current.backendSessionId !== null || current.agentType || current.workspaceId !== initial.workspaceId) {
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
