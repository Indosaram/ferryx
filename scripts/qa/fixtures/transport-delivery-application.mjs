/** Real PTY application. Importing does not attach stdin or OS handlers. */
import { StringDecoder } from 'node:string_decoder';
import { pathToFileURL } from 'node:url';

export const APPLICATION_PROTOCOL = 'ferryx-transport-delivery-application/1';
const keys = ['hostId', 'ownerId', 'epoch', 'backendSessionId'];
const sameTarget = (a, b) => keys.every((key) => a?.[key] === b?.[key]);
const nonempty = (value) => typeof value === 'string' && value.length > 0;
function check(condition, code) {
  if (!condition) throw Object.assign(new Error(code), { code });
}

/** IO must be EventEmitter-style stdin/signals and writable stdout.
 * Configuration is read from stdin after spawn, so target comes from the real
 * spawn reply, never an assumed future backend ID. Only handlers count signals.
 */
export function startDeliveryApplication({ runId, stdin, stdout, signals,
  platform = process.platform }) {
  check(nonempty(runId), 'RUN_ID_REQUIRED');
  let config;
  let armed;
  let stopped = false;
  let writeCount = 0;
  let signalCount = 0;
  let input = '';
  const decoder = new StringDecoder('utf8');
  const consumed = new Set();
  const nonceCounts = new Map();
  const emit = (value) => stdout.write(`${JSON.stringify({ protocol: APPLICATION_PROTOCOL,
    runId, ...value })}\n`);
  const counters = (nonce) => ({ writeCount, signalCount, nonceCount: nonceCounts.get(nonce) ?? 0 });
  const fail = (code) => emit({ event: 'error', code });
  const receipt = (op, event, extra = {}) => emit({ event, target: config.target,
    operationId: op.id, nonce: op.nonce, ...counters(op.nonce), ...extra });
  function configure(message) {
    check(!config, 'ALREADY_CONFIGURED');
    check(keys.every((key) => nonempty(message.target?.[key])) &&
      /^(0|[1-9]\d*)$/.test(message.target.epoch), 'EXACT_TARGET_REQUIRED');
    check(Array.isArray(message.operations) && message.operations.length > 0, 'OPERATIONS_REQUIRED');
    const ids = new Set();
    const nonces = new Set();
    for (const op of message.operations) {
      check(nonempty(op.id) && nonempty(op.nonce) && !ids.has(op.id) && !nonces.has(op.nonce),
        'UNIQUE_OPERATION_NONCE_REQUIRED');
      check(op.kind === 'write' || op.kind === 'signal', 'OPERATION_KIND_REQUIRED');
      if (op.kind === 'signal') {
        check(platform !== 'win32', 'SIGNAL_UNSUPPORTED_PLATFORM');
        check(['SIGINT', 'SIGTERM'].includes(op.signal), 'SIGNAL_UNSUPPORTED');
      }
      ids.add(op.id);
      nonces.add(op.nonce);
    }
    config = structuredClone({ target: message.target, operations: message.operations });
    emit({ event: 'ready', target: config.target, writeCount, signalCount, nonceCount: 0,
      signalSupport: platform === 'win32' ? 'unsupported' : 'posix-handlers' });
  }
  function message(value) {
    check(value.protocol === APPLICATION_PROTOCOL && value.runId === runId, 'RUN_PROTOCOL_MISMATCH');
    if (value.type === 'configure') { configure(value); return; }
    check(config, 'CONFIGURATION_REQUIRED');
    check(sameTarget(value.target, config.target), 'TARGET_MISMATCH');
    const op = config.operations.find((entry) => entry.id === value.operationId && entry.nonce === value.nonce);
    check(op, 'OPERATION_NONCE_MISMATCH');
    if (value.type === 'write') {
      check(op.kind === 'write' && typeof value.payload === 'string', 'WRITE_PAYLOAD_REQUIRED');
      const duplicate = (nonceCounts.get(op.nonce) ?? 0) > 0;
      writeCount++;
      nonceCounts.set(op.nonce, (nonceCounts.get(op.nonce) ?? 0) + 1);
      receipt(op, 'write-consumed', { payload: value.payload, duplicate });
    } else if (value.type === 'arm-signal') {
      check(platform !== 'win32', 'SIGNAL_UNSUPPORTED_PLATFORM');
      check(op.kind === 'signal' && value.signal === op.signal, 'SIGNAL_OPERATION_MISMATCH');
      check(!armed && !consumed.has(op.id), 'SIGNAL_ALREADY_ARMED_OR_CONSUMED');
      armed = op;
      receipt(op, 'signal-armed', { signal: op.signal });
    } else check(false, 'MESSAGE_TYPE_UNSUPPORTED');
  }
  function onData(bytes) {
    if (stopped) return;
    input += decoder.write(Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes));
    if (Buffer.byteLength(input) > 1024 * 1024) {
      fail('INPUT_FRAME_TOO_LARGE');
      stop();
      return;
    }
    let end;
    while ((end = input.indexOf('\n')) !== -1) {
      const line = input.slice(0, end).replace(/\r$/, '');
      input = input.slice(end + 1);
      try { message(JSON.parse(line)); }
      catch (error) { fail(error.code ?? 'INVALID_JSON'); }
    }
  }
  function onSignal(name) {
    if (stopped) return;
    signalCount++;
    if (!armed || armed.signal !== name) {
      emit({ event: 'uncorrelated-signal', signal: name, target: config?.target, writeCount, signalCount });
      return;
    }
    const op = armed;
    armed = undefined;
    consumed.add(op.id);
    nonceCounts.set(op.nonce, (nonceCounts.get(op.nonce) ?? 0) + 1);
    receipt(op, 'signal-consumed', { signal: name });
  }
  const onInt = () => onSignal('SIGINT');
  const onTerm = () => onSignal('SIGTERM');
  function stop() {
    if (stopped) return;
    stopped = true;
    stdin.off('data', onData);
    stdin.off('end', onEnd);
    if (platform !== 'win32') {
      signals.off('SIGINT', onInt);
      signals.off('SIGTERM', onTerm);
    }
    armed = undefined;
  }
  function onEnd() {
    if (input.length || decoder.end().length) fail('INCOMPLETE_INPUT_FRAME');
    stop();
  }
  stdin.on('data', onData);
  stdin.on('end', onEnd);
  if (platform !== 'win32') {
    signals.on('SIGINT', onInt);
    signals.on('SIGTERM', onTerm);
  }
  return { stop };
}

/** Decode consumption and uncorrelated handler records, never ready/arm/error or
 * terminal-echoed input. Caller must also fence runId for its provisioned run.
 */
export function decodeDeliveryApplicationRecord(line, runId) {
  if (!line.startsWith('{')) return null;
  let record;
  try { record = JSON.parse(line); } catch { return null; }
  if (record.protocol !== APPLICATION_PROTOCOL || record.runId !== runId ||
    !['write-consumed', 'signal-consumed', 'uncorrelated-signal'].includes(record.event)) return null;
  return record;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv.length !== 4 || process.argv[2] !== '--run-id' || !nonempty(process.argv[3])) {
    process.stderr.write(`${JSON.stringify({ code: 'ARGV_REQUIRED',
      usage: 'node transport-delivery-application.mjs --run-id <recorded-run-id>' })}\n`);
    process.exitCode = 2;
  } else {
    startDeliveryApplication({ runId: process.argv[3], stdin: process.stdin,
      stdout: process.stdout, signals: process });
    process.stdin.resume();
  }
}
