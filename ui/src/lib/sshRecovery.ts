import { getTerminalRemoteStatus, listTerminalSessions, onTerminalRemoteStatus } from "./tauri";
import type { RemoteSessionStatusResponse, SshRecoveryStatus, TerminalRemoteStatus, TerminalSession } from "./types";
import type { WorkspaceAction } from "../state/workspaceStore";

/** Subscribe first; status snapshots may never overwrite a newer stream observation. */
export function startSshRecovery(options: {
  sessions: TerminalSession[];
  dispatch: (action: WorkspaceAction) => void;
  onError: (error: unknown) => void;
  subscribe?: typeof onTerminalRemoteStatus;
  status?: (id: string) => Promise<RemoteSessionStatusResponse>;
  list?: () => Promise<Array<{ sessionId: string; daemonEpoch?: string | null }>>;
}) {
  let stopped = false;
  let unlisten: (() => void) | undefined;
  const latest = new Map<string, SshRecoveryStatus>();
  const ids = new Set(options.sessions.map(s => s.backendSessionId).filter((id): id is string => id !== null));
  const list = options.list ?? listTerminalSessions;
  const apply = (status: SshRecoveryStatus, daemonEpoch?: string | null) => {
    if (!stopped) options.dispatch({ type: "SESSION_REMOTE_STATUS", status, daemonEpoch });
  };
  const reconcile = async (status: SshRecoveryStatus) => {
    try {
      const sessions = await list();
      if (latest.get(status.sessionId) === status) {
        apply(status, sessions.find(s => s.sessionId === status.sessionId)?.daemonEpoch);
      }
    } catch (error) { if (!stopped) options.onError(error); }
  };
  const subscribed = (options.subscribe ?? onTerminalRemoteStatus)((status: TerminalRemoteStatus) => {
    if (stopped || !ids.has(status.sessionId)) return;
    latest.set(status.sessionId, status);
    apply(status);
    void reconcile(status);
  }).then(dispose => { if (stopped) dispose(); else unlisten = dispose; });
  const ready = subscribed.then(async () => {
    if (stopped) return;
    await Promise.all([...ids].map(async sessionId => {
      const before = latest.get(sessionId);
      try {
        const result = await (options.status ?? getTerminalRemoteStatus)(sessionId);
        if (stopped || latest.get(sessionId) !== before) return;
        const status: SshRecoveryStatus = result.details
          ? { sessionId, state: result.details.state, generation: result.details.generation, failure: result.details.failure, replayGap: result.details.replayGap }
          : { sessionId, state: result.legacyDirectSsh ? "legacyLost" : "missing", generation: 0, failure: null, replayGap: null };
        latest.set(sessionId, status);
        apply(status);
        await reconcile(status);
      } catch (error) { if (!stopped) options.onError(error); }
    }));
  });
  return { subscribed, ready, stop: () => { stopped = true; unlisten?.(); } };
}
