/** Read-only collector over connectTransportHelper.call('pty.read', params).
 * decodeRecord belongs to the provisioned application, not the helper protocol.
 * It maps one complete output line to { operationId, nonce, nonceCount,
 * writeCount, signalCount, target }, or null for unrelated application output.
 * Never feed write RPC replies into this boundary. No mutation is sent here.
 */
import { StringDecoder } from 'node:string_decoder';

const fields = ['hostId', 'ownerId', 'epoch', 'backendSessionId'];
const decimal = (value) => typeof value === 'string' && /^(0|[1-9]\d*)$/.test(value);
const count = (value) => Number.isSafeInteger(value) && value >= 0;
const sameTarget = (a, b) => fields.every((key) => a?.[key] === b?.[key]);
function requireValue(condition, code) {
  if (!condition) throw Object.assign(new Error(code), { code });
}

export function createTransportDeliveryObserver({ helper, target, operation, cursor,
  producerContract, runId, decodeRecord, signal }) {
  requireValue(typeof helper?.call === 'function', 'PROVISIONED_HELPER_REQUIRED');
  requireValue(fields.every((key) => typeof target?.[key] === 'string' && target[key].length) &&
    decimal(target.epoch), 'EXACT_TARGET_REQUIRED');
  requireValue(decimal(cursor), 'RECORDED_CURSOR_REQUIRED');
  requireValue(operation?.kind === 'write' && typeof operation.id === 'string' && operation.id.length &&
    typeof operation.nonce === 'string' && operation.nonce.length &&
    sameTarget(operation.target, target) && operation.cursor === cursor &&
    count(operation.before?.writeCount) && count(operation.before?.nonceCount) &&
    count(operation.before?.signalCount), 'RECORDED_WRITE_OPERATION_REQUIRED');
  requireValue(typeof producerContract === 'string' && producerContract.length &&
    typeof runId === 'string' && runId.length &&
    typeof decodeRecord === 'function', 'APPLICATION_OUTPUT_CONTRACT_REQUIRED');
  const identity = Object.freeze(Object.fromEntries(fields.map((key) => [key, target[key]])));
  const recorded = structuredClone(operation);
  let current = cursor;
  let pending = false;
  let cancelled = false;
  let broken = false;
  let partial = '';
  let firstCursor;
  const decoder = new StringDecoder('utf8');
  const chunks = new Map();
  const observations = [];
  const signalObservations = [];
  let lastWriteCount = recorded.before.writeCount;
  let lastNonceCount = recorded.before.nonceCount;
  let lastSignalCount = recorded.before.signalCount;
  const check = () => {
    requireValue(!cancelled && !signal?.aborted, 'DELIVERY_CANCELLED');
    requireValue(!broken, 'DELIVERY_STREAM_INVALID');
  };

  return {
    cancel() { cancelled = true; },
    snapshot() {
      return { status: 'incomplete', target: identity, operation: structuredClone(recorded),
        initialCursor: cursor, cursor: current, producerContract, runId,
        observations: structuredClone(observations),
        signalObservations: structuredClone(signalObservations),
        writeDeliveryObserved: observations.length > 0,
        signalCoverage: false, cancelled: cancelled || Boolean(signal?.aborted),
        streamInvalid: broken,
        remaining: ['actual-delivery-before-ack-loss-boundary',
          'complete-owner-mutation-window', 'operation-correlated-signal-producer'] };
    },
    async read({ waitMs = 0 } = {}) {
      check();
      requireValue(!pending, 'DELIVERY_SINGLE_FLIGHT');
      requireValue(Number.isInteger(waitMs) && waitMs >= 0 && waitMs <= 10000, 'INVALID_WAIT');
      pending = true;
      const requestedCursor = current;
      try {
        const reply = await helper.call('pty.read', { target: identity, cursor: requestedCursor, waitMs });
        // Existing helper call owns its bounded deadline. Cancellation fences a late reply;
        // this collector does not close a connection shared with another observer.
        check();
        requireValue(reply?.ok === true, 'HELPER_READ_REQUIRED');
        const data = reply.data;
        requireValue(sameTarget(data?.target, identity), 'STALE_DELIVERY_TARGET');
        requireValue(data.gap === false, 'DELIVERY_OUTPUT_GAP');
        requireValue(decimal(data.cursor) && BigInt(data.cursor) >= BigInt(current) &&
          Array.isArray(data.chunks), 'STALE_DELIVERY_CURSOR');
        for (const chunk of data.chunks) {
          requireValue(decimal(chunk.cursor), 'INVALID_CHUNK_CURSOR');
          if (Number.isSafeInteger(chunk.sequence)) {
            requireValue(BigInt(chunk.sequence) === BigInt(chunk.cursor), 'CHUNK_SEQUENCE_CONFLICT');
          }
          requireValue(typeof chunk.data === 'string' || Array.isArray(chunk.bytes), 'OUTPUT_BYTES_REQUIRED');
          const bytes = typeof chunk.data === 'string' ? Buffer.from(chunk.data, 'base64') : Buffer.from(chunk.bytes);
          if (chunks.has(chunk.cursor)) {
            requireValue(chunks.get(chunk.cursor).equals(bytes), 'DUPLICATE_CHUNK_CONFLICT');
            continue;
          }
          requireValue(BigInt(chunk.cursor) === BigInt(current) + 1n, 'STALE_DELIVERY_CURSOR');
          chunks.set(chunk.cursor, bytes);
          firstCursor ??= chunk.cursor;
          current = chunk.cursor;
          partial += decoder.write(bytes);
          requireValue(Buffer.byteLength(partial) <= 1024 * 1024, 'APPLICATION_RECORD_TOO_LARGE');
          let end;
          while ((end = partial.indexOf('\n')) !== -1) {
            const line = partial.slice(0, end).replace(/\r$/, '');
            partial = partial.slice(end + 1);
            const record = decodeRecord(line);
            if (record != null) {
              requireValue(record.protocol === producerContract && record.runId === runId,
                'STALE_APPLICATION_RUN');
              requireValue(sameTarget(record.target, identity), 'STALE_APPLICATION_TARGET');
              if (record.event === 'uncorrelated-signal') {
                requireValue(count(record.writeCount) && count(record.signalCount) &&
                  record.writeCount >= lastWriteCount && record.signalCount > lastSignalCount,
                'APPLICATION_COUNTER_REGRESSION');
                signalObservations.push({ firstCursor, cursor: current, record: structuredClone(record) });
                lastSignalCount = record.signalCount;
                requireValue(false, 'UNEXPECTED_SIGNAL_DELIVERY');
              }
              requireValue(count(record.writeCount) && count(record.nonceCount) &&
                count(record.signalCount), 'APPLICATION_COUNTERS_REQUIRED');
              requireValue(record.writeCount >= lastWriteCount && record.signalCount >= lastSignalCount,
                'APPLICATION_COUNTER_REGRESSION');
              lastWriteCount = record.writeCount;
              lastSignalCount = record.signalCount;
              if (record.operationId === recorded.id && record.nonce === recorded.nonce) {
                requireValue(record.nonceCount >= lastNonceCount, 'APPLICATION_COUNTER_REGRESSION');
                lastNonceCount = record.nonceCount;
                observations.push({ source: 'helper-pty-read-application-output',
                  target: identity, operationId: recorded.id, nonce: recorded.nonce,
                  firstCursor, cursor: current, record: structuredClone(record) });
                requireValue(record.writeCount === recorded.before.writeCount + 1 &&
                  record.nonceCount === recorded.before.nonceCount + 1 &&
                  record.signalCount === recorded.before.signalCount, 'DELIVERY_COUNTER_CONFLICT');
              }
            }
            firstCursor = partial.length ? current : undefined;
          }
        }
        requireValue(data.cursor === current, 'DELIVERY_CURSOR_WITHOUT_OUTPUT');
        return this.snapshot();
      } catch (error) {
        if (error.code !== 'DELIVERY_CANCELLED') broken = true;
        throw error;
      } finally { pending = false; }
    },
  };
}
