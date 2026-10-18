/** In-process adapter over existing daemon/helper RPC. No observer servers. */
import { EventEmitter } from 'node:events';
import { subscribe, TransportBarrier } from './transport-provider.mjs';
import { startTransportSshDriver } from './transport-ssh-driver.mjs';

export const UNSUPPORTED_OBSERVATIONS = Object.freeze({
  'actor-ownership': 'Task6 in-process Runtime owner/hub assertions; not binary RPC evidence',
  'pending-dial-authority': 'Task6 held loopback handshake plus authority cancellation/completion assertions',
  'direct-relay-selection': 'Existing isolated MachineClient route fixture must establish selected route',
  'backend-unavailable': 'Existing App/reducer fixture must observe unavailable without create',
  'descriptor-cold-restore': 'Existing App persistence fixture must drive actual cold restoration',
  'uncertain-delivery': 'Real owner application nonce/signal counter joined to delivery-before-ack-loss boundary',
  'zero-create-replay': 'Complete owner request/delivery window; local adapter calls are not global audit',
  '21-network-refusals': 'Route fixture must count 21 actual candidate dials independently of SSH retry callbacks',
  'validated-geometry': 'Owner receipt must join latest validated generation and target to actual 113x37 convergence',
  'real-owner-nonce-delivery': 'Actual helper pty.read output with owner nonce/write/signal counters; RPC acknowledgment is not delivery',
  'native-presentation': 'Native provider owned by st_01a13737',
  'real-owner-helper-output': 'Verifier provisions real isolated helper endpoint and target from actual spawn RPC',
  'daemon-ssh-status-event-subscription': 'Control client exposes RPC replies, not remote status event subscription; candidate callback fixture remains separate',
});

export function startTransportObserver({ control, helper, selection, timeoutMs, signal }) {
  const events = new EventEmitter();
  const history = [];
  let ssh;
  const emit = (receipt) => {
    history.push(receipt);
    events.emit('receipt', receipt);
  };
  async function daemon(payload) {
    const wait = subscribe(events, 'rpc-completed', (r) => r.payload === payload, timeoutMs, signal);
    control.call(payload).then((result) => events.emit('rpc-completed', { payload, result }),
      (error) => events.emit('rpc-completed', { payload, error }));
    try {
      const receipt = await wait.promise;
      if (receipt.error) throw receipt.error;
      if (receipt.result.type === 'error') throw new TransportBarrier(receipt.result.code, receipt.result.message);
      emit({ kind: 'rpc-response', payload, result: receipt.result, source: 'real-daemon-rpc' });
      return receipt.result;
    } finally { wait.cancel(); }
  }
  return {
    events, history, daemon,
    unsupported(requirement) {
      return { status: 'unverified', requirement, required: UNSUPPORTED_OBSERVATIONS[requirement] ?? requirement };
    },
    async snapshot() {
      const sessionId = selection.backendSessionId;
      const description = await daemon({ type: 'describeSession', sessionId });
      if (description.type !== 'describeSessionOk') throw new TransportBarrier('DESCRIBE_SESSION_REQUIRED', sessionId);
      const details = await daemon({ type: 'remoteSessionDetails', sessionId });
      const evidence = { description, details };
      if (selection.kind === 'paired') evidence.descriptor = await daemon({ type: 'pairedTerminalDescriptor', sessionId });
      if (helper && selection.helperTarget) {
        evidence.helper = await helper.call('pty.describe', { target: selection.helperTarget });
      }
      return evidence;
    },
    async helperRead(params) {
      if (!helper || !selection.helperTarget) return this.unsupported('real-owner-helper-output');
      const reply = await helper.call('pty.read', { ...params, target: selection.helperTarget });
      if (!reply.ok) throw new TransportBarrier('HELPER_READ_FAILED', reply.error);
      return { source: 'real-helper-rpc', reply, text: reply.data.chunks.map((chunk) =>
        Buffer.from(chunk.data ?? chunk.bytes, chunk.data !== undefined ? 'base64' : undefined).toString('utf8')).join('') };
    },
    async mutate(op, params) {
      const sessionId = selection.backendSessionId;
      if (op === 'write') return daemon({ type: 'write', sessionId, data: params.data });
      if (op === 'signal') return daemon({ type: 'signal', sessionId, signal: params.signal });
      if (op === 'resize') return daemon({ type: 'resize', sessionId, cols: params.cols, rows: params.rows });
      throw new TransportBarrier('MUTATION_OPERATION_UNKNOWN', op);
    },
    startSsh(count) {
      if (ssh) throw new TransportBarrier('SSH_OWNER_ALREADY_STARTED', selection.backendSessionId);
      ssh = startTransportSshDriver({ control: { call: daemon }, events, session: selection.session,
        emit, initialFailures: count });
      return ssh;
    },
    revoke() {
      return daemon({ type: 'pairedHostRevoke', hostId: selection.hostId, generation: selection.authorityGeneration });
    },
    close() { ssh?.stop(); },
  };
}
