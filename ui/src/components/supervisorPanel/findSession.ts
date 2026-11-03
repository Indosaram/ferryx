import type { WorkspaceState } from '../../state/workspaceStore';

/** Finds the open pane whose daemon session is the `<hostId>:<backendSessionId>` ref. */
export function findSessionByBackendId(
  sessionRef: string,
  states: Iterable<readonly [string, WorkspaceState]>,
): { workspaceId: string; sessionId: string } | null {
  const separator = sessionRef.indexOf(':');
  const backendId = separator < 0 ? sessionRef : sessionRef.slice(separator + 1);
  if (!backendId) return null;
  for (const [workspaceId, state] of states) {
    for (const session of Object.values(state.sessions)) {
      if (session.backendSessionId === backendId) return { workspaceId, sessionId: session.id };
    }
  }
  return null;
}
