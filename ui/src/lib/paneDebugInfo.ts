import type { TerminalSession } from "./types";
import { observePaneLiveness, observePaneLivenessAsync } from "./paneLiveness";

export async function formatPaneDebugInfoAsync(
  leafId: string | null,
  session: TerminalSession | null | undefined,
): Promise<string> {
  const backendId = session?.backendSessionId ?? session?.id;
  const observation = backendId
    ? await observePaneLivenessAsync(backendId, {
        daemonEpoch: session?.daemonEpoch ?? null,
        suspended: session?.processState === "suspended" ? true : session?.processState ? false : null,
      })
    : null;
  const liveness = observation?.verdict ?? "UNKNOWN";

  return JSON.stringify({
    leafId,
    sessionId: session?.id ?? null,
    backendSessionId: session?.backendSessionId ?? null,
    daemonEpoch: session?.daemonEpoch ?? null,
    workspaceId: session?.workspaceId ?? null,
    cwd: session?.cwd ?? null,
    lifecycle: session?.lifecycle ?? null,
    processState: session?.processState ?? null,
    remoteConnectionState: session?.remoteConnectionState ?? null,
    remoteGeneration: session?.remoteGeneration ?? null,
    agentType: session?.agentType ?? null,
    agentSessionId: session?.agentSessionId ?? null,
    liveness,
    nativeSnapshotDeadlineFired: observation?.nativeSnapshotDeadlineFired ?? null,
  });
}

/** Identity triad plus the epoch/transport state that decide which daemon route a pane is bound to. */
export function formatPaneDebugInfo(leafId: string | null, session: TerminalSession | null | undefined): string {
  const backendId = session?.backendSessionId ?? session?.id;
  const liveness = backendId
    ? observePaneLiveness(backendId, {
        daemonEpoch: session?.daemonEpoch ?? null,
        suspended: session?.processState === "suspended" ? true : session?.processState ? false : null,
      })
    : "UNKNOWN";

  return JSON.stringify({
    leafId,
    sessionId: session?.id ?? null,
    backendSessionId: session?.backendSessionId ?? null,
    daemonEpoch: session?.daemonEpoch ?? null,
    workspaceId: session?.workspaceId ?? null,
    cwd: session?.cwd ?? null,
    lifecycle: session?.lifecycle ?? null,
    processState: session?.processState ?? null,
    remoteConnectionState: session?.remoteConnectionState ?? null,
    remoteGeneration: session?.remoteGeneration ?? null,
    agentType: session?.agentType ?? null,
    agentSessionId: session?.agentSessionId ?? null,
    liveness,
  });
}
