import { getTerminalRemoteStatus, isStructuredIpcError, listTerminalSessions, onTerminalRemoteStatus, retryTerminalRemoteSession } from "./tauri";
import type { RemoteSessionStatusResponse, SshRecoveryStatus, TerminalRemoteStatus, TerminalSession } from "./types";
import type { WorkspaceAction } from "../state/workspaceStore";

/** Subscribe first; status snapshots may never overwrite a newer stream observation. */
export function startSshRecovery(options: {
  sessions: TerminalSession[];
  dispatch: (action: WorkspaceAction) => void;
  onError: (error: unknown) => void;
  subscribe?: typeof onTerminalRemoteStatus;
  status?: (id: string) => Promise<RemoteSessionStatusResponse>;
  retry?: (id: string) => Promise<unknown>;
  list?: () => Promise<Array<{ sessionId: string; daemonEpoch?: string | null }>>;
  rearms?: Map<string, number>;
}) {
  let stopped = false;
  let unlisten: (() => void) | undefined;
  const latest = new Map<string, SshRecoveryStatus>();
  const ids = new Set(options.sessions.map(s => s.backendSessionId).filter((id): id is string => id !== null));
  const list = options.list ?? listTerminalSessions;
  const probe = options.status ?? getTerminalRemoteStatus;
  const retry = options.retry ?? retryTerminalRemoteSession;
  const toStatus = (sessionId: string, result: RemoteSessionStatusResponse): SshRecoveryStatus =>
    result.details
      ? { sessionId, state: result.details.state, generation: result.details.generation, failure: result.details.failure, replayGap: result.details.replayGap }
      : { sessionId, state: result.legacyDirectSsh ? "legacyLost" : "missing", generation: 0, failure: null, replayGap: null };
  /**
   * Automatic re-arms allowed per outage. Current daemons keep redialing transport outages with a
   * capped backoff, but a session can still sit in `disconnected` when an older daemon (before an
   * upgrade handover) spent its retry budget, or when a non-transport failure stopped the loop.
   * Re-arm on each observed park — the daemon emits one per park, so the cadence is its own — and
   * stop after a bounded number of them so a persistent failure cannot keep dialing the host.
   */
  const MAX_AUTO_REARMS = 5;
  const rearms = options.rearms ?? new Map<string, number>();
  const rearm = async (sessionId: string) => {
    const used = rearms.get(sessionId) ?? 0;
    if (stopped || used >= MAX_AUTO_REARMS) return false;
    rearms.set(sessionId, used + 1);
    try {
      await retry(sessionId);
    } catch (error) {
      if (!isStructuredIpcError(error)) throw error;
    }
    return true;
  };
  const apply = (status: SshRecoveryStatus, daemonEpoch?: string | null) => {
    if (status.state === "connected") rearms.delete(status.sessionId);
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
  /**
   * Re-arm a parked transport and re-probe so the store settles on the recovered state instead of
   * the `disconnected` snapshot. A newer observation for the session wins and this probe is dropped.
   */
  const rearmAndReprobe = async (sessionId: string, before: SshRecoveryStatus) => {
    try {
      if (!(await rearm(sessionId))) return;
      if (stopped || latest.get(sessionId) !== before) return;
      const result = await probe(sessionId);
      if (stopped || latest.get(sessionId) !== before) return;
      const status = toStatus(sessionId, result);
      latest.set(sessionId, status);
      apply(status);
      await reconcile(status);
    } catch (error) { if (!stopped) options.onError(error); }
  };
  const subscribed = (options.subscribe ?? onTerminalRemoteStatus)((status: TerminalRemoteStatus) => {
    if (stopped || !ids.has(status.sessionId)) return;
    const previous = latest.get(status.sessionId)?.state;
    latest.set(status.sessionId, status);
    apply(status);
    void reconcile(status);
    // One park, one re-arm: repeats of the same `disconnected` observation are not new outages.
    if (status.state === "disconnected" && previous !== "disconnected") {
      void rearmAndReprobe(status.sessionId, status);
    }
  }).then(dispose => { if (stopped) dispose(); else unlisten = dispose; });
  const ready = subscribed.then(async () => {
    if (stopped) return;
    await Promise.all([...ids].map(async sessionId => {
      const before = latest.get(sessionId);
      try {
        let result = await probe(sessionId);
        if (stopped || latest.get(sessionId) !== before) return;
        // No details and no legacy marker means the daemon we asked does not know this session,
        // which is transient: a draining predecessor may still own it, or restore has not finished.
        // Ask the daemon to retry the session once and re-probe, so the startup snapshot records the
        // settled answer instead of a `missing` that never kills the remote process anyway.
        let didStartupRearm = false;
        if (!result.details && !result.legacyDirectSsh) {
          didStartupRearm = true;
          await rearm(sessionId);
          if (stopped || latest.get(sessionId) !== before) return;
          result = await probe(sessionId);
          if (stopped || latest.get(sessionId) !== before) return;
        }
        const status = toStatus(sessionId, result);
        latest.set(sessionId, status);
        apply(status);
        // The startup snapshot is the moment a parked transport is most likely to be seen again, so
        // re-arm it here rather than leaving the pane dead until a manual Reconnect.
        if (!didStartupRearm && status.state === "disconnected" && before?.state !== "disconnected") {
          await rearmAndReprobe(sessionId, status);
          return;
        }
        await reconcile(status);
      } catch (error) { if (!stopped) options.onError(error); }
    }));
  });
  return { subscribed, ready, stop: () => { stopped = true; unlisten?.(); } };
}
