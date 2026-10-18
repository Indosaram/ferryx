/** Run the actual candidate SSH recovery owner over real daemon RPC.
 * Bun loads the checkout's TypeScript module; no replacement retry loop here.
 */
import { startSshRecovery } from '../../../ui/src/lib/sshRecovery.ts';

export function startTransportSshDriver({ control, events, session, emit, initialFailures = 0 }) {
  let failures = initialFailures;
  let failureCount = 0;
  const call = async (payload) => {
    const result = await control.call(payload);
    if (result.type === 'error') throw { code: result.code, message: result.message, details: result.details };
    return result;
  };
  const owner = startSshRecovery({
    sessions: [session],
    dispatch(action) { emit({ kind: 'frontend-action', action }); },
    onError(error) { emit({ kind: 'frontend-error', code: error?.code, message: error?.message }); },
    async subscribe(listener) {
      const receive = (receipt) => {
        if (receipt.kind === 'ssh-status' && receipt.status.sessionId === session.backendSessionId) {
          listener(receipt.status);
        }
      };
      events.on('receipt', receive);
      return () => events.off('receipt', receive);
    },
    async status(sessionId) {
      const result = await call({ type: 'remoteSessionDetails', sessionId });
      return { details: result.details, legacyDirectSsh: result.legacyDirectSsh };
    },
    async list() {
      const result = await call({ type: 'listSessions' });
      return result.sessions;
    },
    async retry(sessionId) {
      emit({ kind: 'request', op: 'retryRemoteSession', backendSessionId: sessionId });
      if (failures > 0) {
        failures--;
        // Exact ingress fault: the actual product owner called its retry seam.
        // Count here, not when the fixture was merely configured.
        emit({ kind: 'ipc-rearm-failure', op: 'retryRemoteSession',
          outcome: 'injected-ipc-failure', backendSessionId: sessionId,
          count: ++failureCount });
        throw { code: 'IPC_UNAVAILABLE', message: 'isolated Task9 ingress fault', details: {} };
      }
      return call({ type: 'retryRemoteSession', sessionId });
    },
  });
  return {
    owner,
    armFailures(count) { failures = count; },
    get failureCount() { return failureCount; },
    stop() { owner.stop(); },
  };
}
