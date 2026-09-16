import {
  describeTerminal,
  spawnTerminalDetailed,
  toIpcError,
  type SpawnTerminalResult,
  type TerminalDescribeResult,
} from "./tauri";
import type { AgentReconnectDependencies } from "./agentReconnect";

/**
 * Extracts conflicting backend session ID from an AGENT_SESSION_CONFLICT error.
 * Uses structured IPC error inspection, NEVER regex-matching message strings.
 * Inspects details.existingSessionId, falling back defensively to details.raw JSON parse.
 */
export function extractConflictingBackendSessionId(error: unknown): string | null {
  const ipcError = toIpcError(error);
  if (ipcError.code !== "AGENT_SESSION_CONFLICT") {
    return null;
  }

  const details = ipcError.details;
  if (details && typeof details === "object") {
    if (typeof details.existingSessionId === "string" && details.existingSessionId.trim() !== "") {
      return details.existingSessionId.trim();
    }

    if (typeof details.raw === "string") {
      try {
        const parsed = JSON.parse(details.raw);
        if (parsed && typeof parsed === "object") {
          if (typeof parsed.existingSessionId === "string" && parsed.existingSessionId.trim() !== "") {
            return parsed.existingSessionId.trim();
          }
          if (
            parsed.details &&
            typeof parsed.details === "object" &&
            typeof parsed.details.existingSessionId === "string" &&
            parsed.details.existingSessionId.trim() !== ""
          ) {
            return parsed.details.existingSessionId.trim();
          }
        }
      } catch {
        // Defensive: ignore JSON parse failure
      }
    }
  }

  return null;
}

export type ConflictAdoptionOptions = {
  describe?: (sessionId: string) => Promise<TerminalDescribeResult | null>;
};

/**
 * Wraps reconnect dependencies so that when spawn fails with AGENT_SESSION_CONFLICT
 * and the owning backend session is live and belongs to the same workspace, the GUI
 * adopts the existing backend session instead of failing.
 */
export function withAgentConflictAdoption(
  base: AgentReconnectDependencies,
  options?: ConflictAdoptionOptions,
): AgentReconnectDependencies {
  const originalSpawn = base.spawn ?? spawnTerminalDetailed;

  return {
    ...base,
    spawn: async (request) => {
      try {
        return await originalSpawn(request);
      } catch (error) {
        const existingSessionId = extractConflictingBackendSessionId(error);
        if (!existingSessionId) {
          throw error;
        }

        const sessions = base.getSessions();
        const doubleBound = Object.values(sessions).some(
          (s) => s.backendSessionId === existingSessionId,
        );
        if (doubleBound) {
          const ipcError = toIpcError(error);
          throw {
            code: "AGENT_SESSION_CONFLICT",
            message:
              "This conversation is already open in another terminal — switch to that tab or close it to resume here.",
            details: ipcError.details,
          };
        }

        let described: TerminalDescribeResult | null = null;
        try {
          // Resolve the describe binding lazily: test doubles that mock "./tauri"
          // without the new export must not break the happy-path spawn wrapper.
          const describe = options?.describe ?? describeTerminal;
          described = await describe(existingSessionId);
        } catch {
          // Wrap describeTerminal failures defensively: adoption is best effort, rethrow original error
          throw error;
        }

        if (!described || !described.running || described.workspaceId !== request.workspaceId) {
          throw error;
        }

        const reconnectingLeaf = Object.values(sessions).find(
          (s) =>
            (request.clientRequestId && s.reconnectRequestId === request.clientRequestId) ||
            (request.startup?.kind === "agentResume" &&
              s.providerSession?.id === request.startup.providerSession.id &&
              s.providerSession?.key === request.startup.providerSession.key),
        );

        // In REBIND_SESSION_BACKEND (ui/src/state/workspaceStore.ts):
        //   daemonEpoch: action.daemonEpoch ?? null
        // If an empty string "" were passed, session.daemonEpoch becomes "", which causes
        // sessionPersistence restore reconciliation to treat it as an epoch mismatch against
        // the live daemon epoch (effectiveLiveEpoch !== persistedEpoch), marking the session exited.
        // Keeping the reconnecting leaf's previous daemonEpoch preserves consistency when available;
        // if absent, empty string "" satisfies the SpawnTerminalResult.daemonEpoch type contract.
        const daemonEpoch = reconnectingLeaf?.daemonEpoch ?? "";

        const adopted: SpawnTerminalResult = {
          sessionId: existingSessionId,
          daemonEpoch,
          session: {
            sessionId: existingSessionId,
            workspaceId: described.workspaceId,
            worktree: described.worktree ?? null,
            cwd: described.cwd ?? null,
            cols: described.cols,
            rows: described.rows,
            running: described.running,
          },
        };

        return adopted;
      }
    },
  };
}
