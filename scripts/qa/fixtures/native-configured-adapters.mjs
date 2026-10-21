import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { posix } from 'node:path';
import { createInterface } from 'node:readline';
import { OwnedProcessLedger, executablePathMatches } from '../herdr-reference-fixtures.mjs';
import { HarnessError, ATTACH_TUPLE_FIELDS, requireSevenTupleReceipt } from '../../lib/qa-scenarios/common-harness.mjs';

const quote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;
const gap = (message) => new HarnessError('NATIVE_SETUP_UNIMPLEMENTED', message);

export async function capturePresentationFailure({ agent, profile, capture, artifact, publish,
  makeSignal = () => AbortSignal.timeout(5_000) }) {
  const signal = makeSignal();
  // Both observations precede teardown and must survive scenario cancellation.
  // A missing snapshot is diagnostic evidence, never a replacement receipt.
  for (const [name, collect] of [
    ['screenshot', () => capture('setup-presentation-failed', signal)],
    ['snapshot', async () => {
      const snapshot = await agent.call('snapshot', { profile }, signal);
      signal.throwIfAborted();
      const path = await artifact('setup-presentation-failed-snapshot.json', Buffer.from(JSON.stringify(snapshot)));
      if (typeof path !== 'string' || !path) throw gap('Presentation diagnostic snapshot artifact path is missing');
      publish({ kind: 'native-launch-artifact', phase: 'setup-presentation-failed-snapshot', path });
    }],
  ]) {
    try { await collect(); }
    catch (error) { publish({ kind: 'native-presentation-diagnostic-failed', diagnostic: name, code: error.code ?? 'DIAGNOSTIC_FAILED' }); }
  }
  // Window inspection gets its own budget even if capture exhausted the first.
  const windowsSignal = makeSignal();
  try {
    const windows = await agent.call('setup-child-windows', { profile, timeoutMs: 5_000 }, windowsSignal);
    windowsSignal.throwIfAborted();
    const path = await artifact('setup-presentation-failed-child-windows.json', Buffer.from(JSON.stringify(windows)));
    if (typeof path !== 'string' || !path) throw gap('Presentation diagnostic child windows artifact path is missing');
    publish({ kind: 'native-launch-artifact', phase: 'setup-presentation-failed-child-windows', path });
  } catch (error) {
    publish({ kind: 'native-presentation-diagnostic-failed', diagnostic: 'child-windows', code: error.code ?? 'DIAGNOSTIC_FAILED' });
  }
}

export async function requestNativeSnapshot({
  agent,
  profile,
  runId,
  operationId,
  guiReceipt,
  targetTuple,
  timeoutMs = 5_000,
  signal,
}) {
  if (!targetTuple) throw gap('Exact target 7-tuple required for native snapshot observation');
  requireSevenTupleReceipt(targetTuple, {}, 'snapshot target');
  if (!guiReceipt?.pid) throw gap('GUI launch receipt is required before native snapshot observation');
  const boundedTimeout = Math.min(Math.max(1, timeoutMs), 5_000);
  const requestId = randomUUID();
  const response = await agent.call('snapshot', {
    profile,
    runId,
    operationId,
    requestId,
    attachTuple: targetTuple,
    timeoutMs: boundedTimeout,
  }, signal);
  if (!response || typeof response !== 'object') throw gap('Native snapshot response is invalid');
  if (response.runId !== runId || response.operationId !== operationId || response.requestId !== requestId) {
    throw gap('Native snapshot response correlation mismatch');
  }
  if (response.producer !== 'ipc-qa-barrier' || response.producerPid !== guiReceipt.pid) {
    throw gap('Native snapshot belongs to wrong producer or PID');
  }
  if (response.status !== 'ok') {
    throw gap(`Native snapshot refused: ${response.code ?? 'UNKNOWN'}`);
  }
  const { snapshot } = response;
  if (!snapshot || typeof snapshot !== 'object') throw gap('Native snapshot payload is missing');
  requireSevenTupleReceipt(snapshot.attachTuple, {}, 'snapshot result');
  for (const field of ATTACH_TUPLE_FIELDS) {
    if (snapshot.attachTuple[field] !== targetTuple[field]) {
      throw gap(`Native snapshot target tuple mismatch on ${field}`);
    }
  }
  return snapshot;
}

// Attaches the spawn-recorded PTY identity to a measured frontend snapshot only
// when it names the same backend session, incarnation and daemon epoch. A
// mismatch or missing join leaves the identity absent so the runner refuses.
export function withJoinedTargetIdentity(snapshot, joined) {
  const tuple = snapshot?.attachTuple;
  if (!joined || !tuple || joined.backendSessionId !== tuple.backendSessionId ||
      joined.incarnation !== tuple.incarnation || joined.daemonEpoch !== tuple.daemonEpoch) return snapshot;
  return { ...snapshot, target: { ...snapshot.target, pid: joined.pid, processStart: joined.processStart,
    executable: joined.executable, backendSessionId: joined.backendSessionId, incarnation: joined.incarnation,
    daemonEpoch: joined.daemonEpoch } };
}

export async function setupNativeTerminal({ agent, mahord, profile, gui, presentation, subscribe, capture, publish, runId, operationId, signal, captureFailure }) {
  const args = { profile, gui };
  let target = await agent.call('setup-target', args, signal);
  await capture('setup-target');
  let hit;
  for (let attempt = 0; attempt <= 8; attempt++) {
    hit = await agent.call('setup-target', { ...args, hwnd: target.hwnd, point: target.point }, signal);
    publish({ kind: 'native-setup-target', attempt, ...hit });
    if (hit.owned) break;
    if (attempt === 8) break;
    if (target.point?.y >= 100 && hit.hitHwnd === target.hwnd) {
      await mahord.click({ x: target.point.x, y: Math.max(0, target.point.y - 60) }, signal);
    } else {
      // Existing Windows window-switching UI; no SSH/UIA focus or input.
      await mahord.hotkey(['Alt', 'Escape'], signal);
    }
    await capture(`setup-focus-${attempt}`);
  }
  if (!hit?.owned) throw gap('Setup target foreground/occlusion refused after window switching');
  if (!(target.point?.y >= 100)) {
    target = await agent.call('setup-target', args, signal);
  }
  await capture('setup-focused-target');
  const cursor = await presentation.cursor({ signal });
  const bound = prearmPresentationBinding({ cursor, runId, operationId, subscribe, signal });
  try {
    let fresh;
    const maxFreshAttempts = target.point?.y >= 100 ? 8 : 0;
    for (let attempt = 0; attempt <= maxFreshAttempts; attempt++) {
      fresh = await agent.call('setup-target', { ...args, hwnd: target.hwnd, point: target.point }, signal);
      publish({ kind: 'native-setup-target', final: true, ...(maxFreshAttempts > 0 ? { attempt } : {}), ...fresh });
      if (fresh.owned || attempt === maxFreshAttempts) break;
      if (fresh.hitHwnd === target.hwnd) {
        await mahord.click({ x: target.point.x, y: Math.max(0, target.point.y - 60) }, signal);
      } else {
        await mahord.hotkey(['Alt', 'Escape'], signal);
      }
    }
    if (!fresh?.owned) throw gap('Setup target lost foreground or became occluded before click');
    // A residual focus race remains between this read-only guard and MahoRD input.
    await mahord.click(target.point, signal);
    let receipt;
    try { receipt = await bound.promise; }
    catch (error) {
      await captureFailure?.();
      throw error;
    }
    requireSevenTupleReceipt(receipt);
    if (receipt.producerPid !== gui.pid) throw gap('Setup presentation belongs to another GUI');
    return receipt;
  } finally { bound.cancel(); }
}

export function prearmPresentationBinding({ cursor, runId, operationId, subscribe, signal }) {
  let dispose = () => {};
  const promise = new Promise((resolveBinding, reject) => {
    const finish = (error, receipt) => { dispose(); error ? reject(error) : resolveBinding(receipt); };
    const abort = () => finish(signal.reason);
    const unlisten = subscribe((event) => {
      if (event.kind === 'receipt-error') { finish(event.error); return; }
      const raw = event.receipt;
      if (event.kind === 'presentation' && event.path === cursor.path && event.lineIndex > cursor.afterLine &&
          raw.runId === runId && raw.operationId === operationId && raw.presented === true &&
          raw.producerComponent === 'surface-host-gpu-render') finish(null, raw);
    });
    dispose = () => { unlisten(); signal.removeEventListener('abort', abort); };
    signal.addEventListener('abort', abort, { once: true });
    if (signal.aborted) abort();
  });
  promise.catch(() => {});
  return { promise, cancel: () => dispose() };
}

// Two-pane focus fixture: the product's own split-down chord (Ctrl+Shift+D off
// macOS; a bare Ctrl+D would reach the shell as EOF) opens a second real pane,
// which takes focus and leaves the recovery target as the previous focus. The
// subscription is armed before the chord; only a presented GPU receipt for a
// different pane of the same GUI settles it.
export async function splitSecondPane({ mahord, platform, presentation, subscribe, target, gui, runId, operationId, capture, signal }) {
  const cursor = await presentation.cursor({ signal });
  let dispose = () => {};
  const second = new Promise((resolveSecond, reject) => {
    const finish = (error, receipt) => { dispose(); error ? reject(error) : resolveSecond(receipt); };
    const abort = () => finish(signal.reason);
    const unlisten = subscribe((event) => {
      if (event.kind === 'receipt-error') { finish(event.error); return; }
      const raw = event.receipt;
      if (event.kind === 'presentation' && event.path === cursor.path && event.lineIndex > cursor.afterLine &&
          raw.runId === runId && raw.operationId === operationId && raw.presented === true &&
          raw.producerComponent === 'surface-host-gpu-render' && raw.producerPid === gui.pid &&
          raw.attachTuple?.paneIdentity && raw.attachTuple.paneIdentity !== target.paneIdentity &&
          raw.attachTuple.backendSessionId !== target.backendSessionId) finish(null, raw);
    });
    dispose = () => { unlisten(); signal.removeEventListener('abort', abort); };
    signal.addEventListener('abort', abort, { once: true });
    if (signal.aborted) abort();
  });
  second.catch(() => {});
  try {
    await mahord.hotkey(platform === 'darwin' ? ['Meta', 'Shift', 'd'] : ['Control', 'Shift', 'd'], signal);
    const receipt = await second;
    requireSevenTupleReceipt(receipt);
    await capture?.('setup-second-pane');
    return receipt;
  } finally { dispose(); }
}

export async function admitOwnedPty({ record, cursor, runId, operationId, candidate, hostId, daemon, platform, probe, recordOwnedPid, signal }) {
  signal?.throwIfAborted();
  const raw = record.raw;
  if (record.path !== cursor.path || !Number.isSafeInteger(record.lineIndex) || record.lineIndex <= cursor.afterLine) {
    throw gap('Owned PTY receipt is stale or lacks physical cursor provenance');
  }
  if (daemon?.role !== 'daemon' || daemon.runId !== runId || daemon.operationId !== operationId ||
      daemon.candidate !== candidate || daemon.hostId !== hostId || !Number.isSafeInteger(daemon.pid) || daemon.pid <= 0 ||
      !daemon.executable || !daemon.processStart) throw gap('Spawn-recorded isolated daemon identity required');
  if (raw.runId !== runId || raw.operationId !== operationId || raw.producerPid !== daemon.pid ||
      raw.producerComponent !== 'terminal-qa-owned-pty-creation' || raw.role !== 'pty' || raw.ownershipComplete !== true ||
      !Number.isSafeInteger(raw.pid) || raw.pid <= 0 || !raw.backendSessionId || !raw.incarnation || !raw.executable || !raw.processStart) {
    throw gap('Owned PTY receipt is refused, incomplete or from another producer');
  }
  const live = await probe({ pid: raw.pid });
  signal?.throwIfAborted();
  if (live.exited !== false || !executablePathMatches(live.executable, raw.executable, platform) || live.processStart !== raw.processStart) {
    throw gap('Owned PTY exact PID probe disagrees with creation identity');
  }
  await recordOwnedPid({ ...raw, candidate, hostId, ownershipPath: record.path, ownershipLineIndex: record.lineIndex });
}

export async function joinOwnedPty({ record, cursor, runId, operationId, candidate, hostId, daemon, gui, binding, platform, probe, recordOwnedPid, signal }) {
  signal?.throwIfAborted();
  const raw = record.raw;
  if (record.path !== cursor.path || !Number.isSafeInteger(record.lineIndex) || record.lineIndex <= cursor.afterLine) {
    throw gap('Owned PTY receipt is stale or lacks physical cursor provenance');
  }
  if (daemon?.role !== 'daemon' || gui?.role !== 'gui' ||
      [daemon, gui].some((owner) => owner.runId !== runId || owner.operationId !== operationId ||
        owner.candidate !== candidate || owner.hostId !== hostId || !Number.isSafeInteger(owner.pid) || owner.pid <= 0 ||
        !owner.executable || !owner.processStart)) throw gap('Spawn-recorded isolated daemon and GUI identities required');
  if (raw.runId !== runId || raw.operationId !== operationId || raw.producerPid !== daemon.pid ||
      raw.producerComponent !== 'terminal-qa-owned-pty-creation' || raw.role !== 'pty' || raw.ownershipComplete !== true ||
      !Number.isSafeInteger(raw.pid) || raw.pid <= 0 || !raw.incarnation || !raw.executable || !raw.processStart) {
    throw gap('Owned PTY receipt is refused, incomplete or from another producer');
  }
  if (binding?.producerPid !== gui.pid || binding.runId !== runId || binding.operationId !== operationId ||
      !ATTACH_TUPLE_FIELDS.every((field) => binding.attachTuple?.[field] !== undefined && binding.attachTuple[field] !== null) ||
      binding.attachTuple.backendSessionId !== raw.backendSessionId || binding.attachTuple.incarnation !== raw.incarnation) {
    throw gap('Actual GUI frontend/backend binding producer is missing or disagrees with owned PTY');
  }
  const live = await probe({ pid: raw.pid });
  signal?.throwIfAborted();
  if (live.exited !== false || !executablePathMatches(live.executable, raw.executable, platform) || live.processStart !== raw.processStart) {
    throw gap('Owned PTY exact PID probe disagrees with creation identity');
  }
  const target = { ...raw, candidate, hostId, daemonEpoch: binding.attachTuple.daemonEpoch,
    attachTuple: binding.attachTuple, ownershipPath: record.path, ownershipLineIndex: record.lineIndex };
  await recordOwnedPid(target);
  return target;
}

export async function joinFixtureChild({ target, fixtureChild, platform, probe, recordOwnedPid, expectedExecutable, expectedRunId, expectedOperationId, signal }) {
  signal?.throwIfAborted();
  if (!target || !Number.isSafeInteger(target.pid) || target.pid <= 0) {
    throw gap('Authoritative PTY root target required to join fixture child');
  }
  if (!fixtureChild || !Number.isSafeInteger(fixtureChild.pid) || fixtureChild.pid <= 0) {
    throw gap('Valid fixture child readiness receipt required');
  }
  if (expectedRunId && fixtureChild.runId !== expectedRunId) {
    throw gap(`Fixture child runId ${fixtureChild.runId} does not match expected ${expectedRunId}`);
  }
  if (expectedOperationId && fixtureChild.operationId !== expectedOperationId) {
    throw gap(`Fixture child operationId ${fixtureChild.operationId} does not match expected ${expectedOperationId}`);
  }
  const childLive = await probe({ pid: fixtureChild.pid, signal });
  signal?.throwIfAborted();
  if (childLive.exited !== false) {
    throw gap('Retained fixture child process exited unexpectedly');
  }
  if (expectedExecutable && !executablePathMatches(childLive.executable, expectedExecutable, platform)) {
    throw gap(`Fixture child executable ${childLive.executable} does not match expected ${expectedExecutable}`);
  }
  if (platform === 'win32') {
    if (childLive.ppid !== target.pid) {
      throw gap(`Windows fixture child (PID ${fixtureChild.pid}, PPID ${childLive.ppid}) is not descendant of PTY root (PID ${target.pid})`);
    }
    if (childLive.processStart < target.processStart) {
      throw gap('Windows fixture child started before PTY root process');
    }
    target.rootPid = target.pid;
    target.descendantPid = fixtureChild.pid;
    target.fixtureChild = { pid: fixtureChild.pid, ppid: childLive.ppid, executable: childLive.executable, processStart: childLive.processStart };
    if (recordOwnedPid) {
      await recordOwnedPid({ pid: fixtureChild.pid, executable: childLive.executable, processStart: childLive.processStart,
        role: 'fixture-child', hostId: target.hostId, candidate: target.candidate });
    }
  } else {
    if (fixtureChild.pid !== target.pid) {
      throw gap(`Unix fixture child (PID ${fixtureChild.pid}) does not match PTY root (PID ${target.pid})`);
    }
    target.rootPid = target.pid;
    target.descendantPid = target.pid;
    target.fixtureChild = { pid: target.pid, executable: childLive.executable, processStart: childLive.processStart };
  }
  return target;
}

// Preserve native-events producer records as their original event payloads.
// Only attach transport provenance; never synthesize missing lifecycle, binding,
// persistence, listener, attach, output, or audit receipts from current state.
export function nativeProducerEvent(raw, source = 'native-events.jsonl') {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw) || typeof raw.kind !== 'string') return null;
  return { ...raw, source };
}

// Join owner-local audit packets only against a frozen registered-owner set.
// Owner sequence numbers remain owner-local; the returned throughSeq is the
// maximum audited owner sequence, never the adapter journal's receipt clock.
export function joinNativeAuditOwners({ auditId, scope, registeredOwners, packets, requiredComponents }) {
  const incomplete = (detail) => ({ complete: false, ingress: false, deliveryCoverage: false,
    deliveries: [], requests: [], scope: 'registered-isolated-owner-set', auditId: auditId ?? null, detail });
  if (typeof auditId !== 'string' || !auditId || scope !== 'registered-isolated-owner-set' ||
      !Array.isArray(registeredOwners) || registeredOwners.length === 0 || !Array.isArray(packets)) {
    return incomplete('Missing audit identity, explicit owner scope, registered owners or owner packets');
  }
  if (Array.isArray(requiredComponents)) {
    for (const comp of requiredComponents) {
      if (!registeredOwners.some((o) => o?.component === comp)) {
        return incomplete(`Missing required producer channel coverage for ${comp}`);
      }
    }
  }
  const ownerIds = new Set();
  for (const owner of registeredOwners) {
    if (!owner || typeof owner.ownerId !== 'string' || !owner.ownerId || ownerIds.has(owner.ownerId) ||
        !Number.isSafeInteger(owner.pid) || owner.pid <= 0 || !owner.executable || !owner.processStart ||
        typeof owner.component !== 'string' || !owner.component) {
      return incomplete('Registered owner manifest lacks unique exact process identities');
    }
    ownerIds.add(owner.ownerId);
  }
  if (packets.length !== ownerIds.size) return incomplete('Owner packet set is incomplete or contains unregistered owners');
  const byId = new Map();
  for (const packet of packets) {
    if (!packet || !ownerIds.has(packet.owner?.ownerId) || byId.has(packet.owner?.ownerId)) {
      return incomplete('Owner packet is duplicate or outside the frozen registered owner set');
    }
    byId.set(packet.owner.ownerId, packet);
  }
  const requests = [];
  const deliveries = [];
  let throughSeq = 0;
  for (const owner of registeredOwners) {
    const packet = byId.get(owner.ownerId);
    if (!packet || packet.auditId !== auditId || packet.complete !== true ||
        packet.owner?.ownerId !== owner.ownerId || packet.owner?.pid !== owner.pid ||
        packet.owner?.executable !== owner.executable || packet.owner?.processStart !== owner.processStart ||
        packet.owner?.component !== owner.component || !Number.isSafeInteger(packet.baselineSeq) ||
        !Number.isSafeInteger(packet.endSeq) || packet.baselineSeq < 0 || packet.endSeq < packet.baselineSeq ||
        !Array.isArray(packet.requests) || !Array.isArray(packet.deliveries)) {
      return incomplete(`Missing, incomplete or identity-mismatched audit packet for owner ${owner.ownerId}`);
    }
    const validRecords = (records, kind) => records.every((record) => record && record.kind === kind &&
      record.ownerId === owner.ownerId && record.pid === owner.pid &&
      record.executable === owner.executable && record.processStart === owner.processStart &&
      Number.isSafeInteger(record.seq) && record.seq > packet.baselineSeq && record.seq <= packet.endSeq);
    if (!validRecords(packet.requests, 'request') || !validRecords(packet.deliveries, 'delivery')) {
      return incomplete(`Out-of-range or foreign request/delivery receipt for owner ${owner.ownerId}`);
    }
    requests.push(...packet.requests);
    deliveries.push(...packet.deliveries);
    throughSeq = Math.max(throughSeq, packet.endSeq);
  }
  const hasIngressChannel = registeredOwners.some((o) => ['terminal-local-pty', 'daemon', 'gui', 'local-pty', 'remote-audit'].includes(o.component));
  const hasDeliveryChannel = registeredOwners.some((o) => ['terminal-local-pty', 'daemon', 'gui', 'local-pty', 'remote-audit'].includes(o.component));
  return { complete: true, ingress: hasIngressChannel, deliveryCoverage: hasDeliveryChannel, scope: 'registered-isolated-owner-set',
    auditId, registeredOwners, requests, deliveries, throughSeq };
}

// Stdio JSONL used by both the existing Maho MCP CLI and the task-owned agent.
// Connect is explicit; importing this file launches nothing.
export function rpcProcess(executable, argv, { mcp = false, signal, processSignal = signal, onEvent = () => {}, spawnProcess = spawn }) {
  const child = spawnProcess(executable, argv, { stdio: ['pipe', 'pipe', 'pipe'], signal: processSignal });
  const pending = new Map();
  let terminalError;
  let next = 0;
  let stderr = '';
  child.stderr.on('data', (bytes) => { stderr += bytes; });
  const lines = createInterface({ input: child.stdout });
  lines.on('line', (line) => {
    let message;
    try { message = JSON.parse(line); } catch { return; } // CLI diagnostics are retained in stderr, not receipt evidence.
    if (message.event) { onEvent(message); return; }
    const item = pending.get(message.id);
    if (!item) return;
    pending.delete(message.id);
    item.dispose();
    if (message.error) item.reject(Object.assign(new Error(message.error.message), { code: message.error.code }));
    else item.resolve(message.result);
  });
  const rejectAll = (error) => {
    terminalError ??= error;
    for (const item of pending.values()) { item.dispose(); item.reject(terminalError); }
    pending.clear();
  };
  child.once('error', rejectAll);
  child.once('close', (code) => rejectAll(new Error(`Native RPC exited ${code}: ${stderr}`)));
  lines.once('close', () => rejectAll(new Error('Native RPC stdout EOF')));
  child.stdin.on('error', rejectAll);
  const write = (packet) => {
    try {
      child.stdin.write(`${JSON.stringify(packet)}\n`, (error) => { if (error) rejectAll(error); });
    } catch (error) { rejectAll(error); }
  };
  const call = (method, args, callSignal = signal) => new Promise((resolveCall, reject) => {
    if (terminalError) { reject(terminalError); return; }
    const id = ++next;
    const abort = () => {
      pending.delete(id);
      callSignal.removeEventListener('abort', abort);
      reject(callSignal.reason);
      if (!mcp && !terminalError) write({ method: 'cancel', args: { id } });
    };
    if (callSignal.aborted) { reject(callSignal.reason); return; }
    pending.set(id, { resolve: resolveCall, reject, dispose: () => callSignal.removeEventListener('abort', abort) });
    callSignal.addEventListener('abort', abort, { once: true });
    write(mcp ? { jsonrpc: '2.0', id, method, params: args } : { id, method, args });
  });
  return { child, call, stderr: () => stderr, close: () => child.stdin.end() };
}

export async function connectNativeMahord({ executable, address, pairingId, nudgeMs, signal, processSignal = signal, spawnProcess = spawn }) {
  // A static Windows desktop emits no frames; nudge keeps the stream and later tool calls alive.
  const nudge = Number.isSafeInteger(nudgeMs) && nudgeMs > 0 ? ['--nudge-ms', String(nudgeMs)] : [];
  const rpc = rpcProcess(executable, ['--host', address, '--pairing-id', pairingId, ...nudge, '--mcp'], { mcp: true, signal, processSignal, spawnProcess });
  let tools;
  try {
    await rpc.call('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'ferryx-native-task9', version: '2' } });
    const listed = await rpc.call('tools/list', {});
    tools = new Set(listed.tools.map((tool) => tool.name));
  } catch (error) {
    try { rpc.close(); } finally { throw error; }
  }
  async function call(name, args, callSignal) {
    if (!tools.has(name)) throw gap(`MahoRD tool ${name} absent from tools/list`);
    const result = await rpc.call('tools/call', { name, arguments: args }, callSignal);
    if (result.isError) throw new Error(result.content.map((item) => item.text ?? '').join('\n'));
    return result;
  }
  return {
    child: rpc.child,
    click: (point, callSignal) => call('remote_mouse_click', { ...point, button: 'left', count: 1, normalized: false }, callSignal),
    type: (text, callSignal) => call('remote_type_text', { text, delay_ms: 0 }, callSignal),
    key: (key, callSignal) => call('remote_key_press', { key }, callSignal),
    hotkey: (keys, callSignal) => call('remote_hotkey', { keys }, callSignal),
    screenshot: async (callSignal) => {
      const result = await call('remote_take_screenshot', { format: 'png' }, callSignal);
      const image = result.content.find((item) => item.type === 'image' && item.mimeType === 'image/png');
      if (!image) throw new Error('MahoRD did not return an actual PNG frame');
      return Buffer.from(image.data, 'base64');
    },
    close: rpc.close,
  };
}

// Explicit config supplies paths/credentials and an existing isolated forwarding
// fixture. This factory supplies concrete shell, process and Maho operations,
// rather than accepting those operations as unexplained callbacks.
export async function traceNativeLaunchPhase(publish, phase, operation) {
  publish({ kind: 'native-launch-phase', phase, outcome: 'started' });
  try {
    const result = await operation();
    publish({ kind: 'native-launch-phase', phase, outcome: 'completed' });
    return result;
  } catch (error) {
    // Do not include command text, tool arguments, credentials, or error messages.
    publish({ kind: 'native-launch-phase', phase, outcome: 'failed', code: error.code });
    throw error;
  }
}

export async function createConfiguredNativeContext(config, { candidate, transport, evidence, runId, mutation, signal, spawnProcess = spawn }) {
  if (!config.sshDestination || !config.remoteAgent || !config.worktree || !config.canonicalClone ||
    !config.worktreeRoot || !config.mahordExecutable || !config.mahordAddress || !config.pairingId) {
    throw gap('Native configuration requires explicit canonical host paths, SSH and MahoRD connection fields');
  }
  if (!transport) throw gap('Existing isolated forwarding transport provider is not configured');
  if (!evidence?.artifact) throw gap('Existing evidence writer artifact(path, bytes) is required');
  if (!config.bun || !config.binary || !config.buildManifest || !config.hostId) {
    throw gap('Native launch requires candidate and host configuration');
  }
  let daemon;
  const subscribers = new Set();
  const journal = [];
  let seq = 0;
  let recordOwnedPid;
  // Adapter-side mirror of every runner-recorded spawn (same shape as the
  // runner ledger entry) so the audit can check its census against spawns.
  const owned = [];
  let receiptFailure;
  let ownershipCursor;
  let guiReceipt;
  let currentBinding = null;
  // Spawn-recorded, exact-PID-probed PTY identity from joinOwnedPty. The
  // frontend snapshot measures geometry only; process identity comes from here.
  let joinedTarget = null;
  const ownershipRecords = [];
  let ownershipAdmission = Promise.resolve();
  let ownershipAdmissionFailure;
  const admittedPtys = new Set();
  const recordCleanupPty = async target => {
    const key = `${target.pid}:${target.processStart}:${target.executable}`;
    if (admittedPtys.has(key)) return;
    await recordOwnedPid(target);
    admittedPtys.add(key);
  };
  const publish = (event) => {
    const normalized = { ...event, seq: ++seq };
    journal.push(normalized);
    for (const subscriber of subscribers) subscriber(normalized);
  };
  const onEvent = (event) => {
    if (event.event === 'gui-launch-subscribed') { publish({ kind: 'gui-launch-subscribed', profile: event.profile }); return; }
    if (event.event === 'receipt-error') { receiptFailure = gap(event.error); publish({ kind: 'receipt-error', error: receiptFailure }); return; }
    if (event.event === 'forwarder-receipt') {
      const { event: _ev, profile: _pr, seq: _seq, ...forwarded } = event;
      if (typeof forwarded.kind === 'string') {
        publish({ ...forwarded, source: 'forwarder-receipt' });
        if (forwarded.kind === 'lost-ack-suppressed') {
          publish({
            kind: 'mutation-outcome',
            mutationId: forwarded.mutationId,
            sessionId: forwarded.sessionId,
            outcome: 'unknown',
            source: 'forwarder-receipt',
          });
        }
      }
      return;
    }
    if (event.event !== 'receipt') return;
    if (event.name === 'native-spawn.jsonl') {
      // A guessed file is not the launcher's spawn-time ownership ledger.
      receiptFailure = gap('GUI/PTY spawn-time ownership must come from the actual launcher, not native-spawn.jsonl');
    } else if (event.name === 'owned-pty-creation.receipt.jsonl') {
      const record = { raw: event.raw, path: event.path, lineIndex: event.lineIndex };
      ownershipRecords.push(record);
      // The agent/cleanup lifetime, not the presentation attempt, owns admission.
      // Keep consuming late spawn receipts until release drains and unwatches.
      ownershipAdmission = ownershipAdmission.then(() => admitOwnedPty({ record, cursor: ownershipCursor,
        runId, operationId: mutation.id, candidate: candidate.stamp, hostId: config.hostId, daemon, platform: config.platform,
        probe: ({ pid }) => host.probePid({ pid, signal: agentLifetime.signal }), recordOwnedPid: recordCleanupPty,
        signal: agentLifetime.signal })).catch(error => {
        ownershipAdmissionFailure ??= error;
        receiptFailure ??= error;
        publish({ kind: 'receipt-error', error });
      });
      publish({ kind: 'owned-pty', raw: event.raw, path: event.path, lineIndex: event.lineIndex });
    } else if (event.name === 'presentation.receipt.jsonl') {
      if (event.raw?.presented === true && event.raw?.attachTuple && currentBinding?.attachTuple &&
          event.raw.attachTuple.paneIdentity === currentBinding.attachTuple.paneIdentity &&
          event.raw.attachTuple.backendSessionId === currentBinding.attachTuple.backendSessionId &&
          event.raw.attachTuple.attemptGeneration >= currentBinding.attachTuple.attemptGeneration) {
        currentBinding = { ...currentBinding, attachTuple: event.raw.attachTuple };
      }
      publish({ kind: 'presentation', receipt: event.raw, source: 'presentation.receipt.jsonl',
        path: event.path, lineIndex: event.lineIndex });
    } else {
      const producerEvent = nativeProducerEvent(event.raw);
      if (producerEvent?.attachTuple && currentBinding?.attachTuple &&
          producerEvent.attachTuple.paneIdentity === currentBinding.attachTuple.paneIdentity &&
          producerEvent.attachTuple.backendSessionId === currentBinding.attachTuple.backendSessionId &&
          producerEvent.attachTuple.attemptGeneration >= currentBinding.attachTuple.attemptGeneration) {
        currentBinding = { ...currentBinding, attachTuple: producerEvent.attachTuple };
      }
      if (producerEvent) publish(producerEvent);
    }
  };
  // No shell fallback for GUI. SSH agent reads evidence/probes isolated processes.
  const remoteCommand = config.platform === 'win32'
    ? `powershell.exe -NoProfile -NonInteractive -Command "& '${config.bun.replaceAll("'", "''")}' '${config.remoteAgent.replaceAll("'", "''")}' --rpc"`
    : `${quote(config.bun)} ${quote(config.remoteAgent)} --rpc`;
  // Calls still use the scenario signal, but the stateful agent must survive it
  // to release the forwarder and clean up spawn-recorded processes. Teardown
  // transfers its existing bounded cancellation budget to this connection.
  const agentLifetime = new AbortController();
  let cleanupSignal;
  const abortAgent = () => agentLifetime.abort(cleanupSignal.reason);
  const agent = rpcProcess(config.sshExecutable ?? 'ssh', ['-T', '-o', 'BatchMode=yes', config.sshDestination, remoteCommand],
    { signal, processSignal: agentLifetime.signal, onEvent, spawnProcess });
  let mahord;
  try {
    mahord = await connectNativeMahord({ executable: config.mahordExecutable, address: config.mahordAddress,
      pairingId: config.pairingId, nudgeMs: config.mahordNudgeMs, signal, processSignal: agentLifetime.signal, spawnProcess });
  } catch (error) {
    try { agent.close(); } finally {
      agentLifetime.abort(error);
      throw error;
    }
  }
  let setup;
  const scenarioDeadline = Date.now() + (config.timeoutMs ?? 120_000);
  const barrierCall = (method, args, s) => {
    const remaining = scenarioDeadline - Date.now();
    if (remaining <= 0) throw gap('Native presentation scenario deadline expired');
    return agent.call(method, { ...args, profile: setup.profile,
      timeoutMs: Math.min(args.timeoutMs ?? remaining, remaining) }, s);
  };
  const presentation = {
    prearm: ({ name = 'presentation', options = {}, signal: s }) => barrierCall('barrier-prearm',
      { name, options: { ...options, deadlineMs: Math.min(options.deadlineMs ?? config.timeoutMs ?? 120_000,
        Math.max(1, scenarioDeadline - Date.now())) } }, s),
    awaitRegistered: ({ name = 'presentation', timeoutMs, signal: s }) => barrierCall('barrier-registered', { name, timeoutMs }, s),
    awaitHeld: ({ name = 'presentation', timeoutMs, signal: s }) => barrierCall('barrier-held', { name, timeoutMs }, s),
    bindBackendSession: ({ name = 'presentation', sessionId, signal: s }) => barrierCall('barrier-bind', { name, sessionId }, s),
    awaitBound: ({ name = 'presentation', timeoutMs, signal: s }) => barrierCall('barrier-bound', { name, timeoutMs }, s),
    release: ({ name = 'presentation', signal: s }) => barrierCall('barrier-release', { name }, s),
    cursor: ({ signal: s }) => barrierCall('barrier-cursor', {}, s),
    awaitPresentation: ({ signal: s, ...args }) => {
      // The caller must supply a separately spawn-recorded GUI PID. Matching
      // this emitter is correlation only; this adapter cannot prove ownership.
      return barrierCall('barrier-presentation', args, s);
    },
  };
  const host = {
    id: config.hostId, platform: config.platform, remote: true, desktopTransport: 'mahord', worktreeRoot: config.worktreeRoot,
    probePid: ({ pid, signal: s }) => agent.call('probe', { pid }, s),
    subscribeExit: ({ pid, processStart, signal: s }) => agent.call('exit', { pid, processStart }, s),
    terminateExact: ({ pid, executable, processStart, tree, signal: s }) => {
      if (tree !== false) throw gap('Process tree termination is forbidden');
      return agent.call('terminate', { pid, executable, processStart }, s);
    },
    async removeProfile({ setup: isolated, signal: s }) {
      const files = await agent.call('collect-launch-evidence', { profile: isolated.profile }, s);
      for (const file of files) {
        if (file.missing) {
          if (file.name === 'receipts/gui-launch.json' && guiReceipt) throw gap('Recorded GUI launch receipt is missing before cleanup');
          continue;
        }
        s.throwIfAborted();
        const path = await evidence.artifact(`native-${runId}-${file.name.replaceAll('/', '-')}`, Buffer.from(file.base64, 'base64'));
        if (typeof path !== 'string' || !path) throw gap('Native cleanup evidence artifact path is missing');
        publish({ kind: 'native-cleanup-artifact', name: file.name, path });
      }
      s.throwIfAborted();
      return agent.call('remove-profile', { profile: isolated.profile }, s);
    },
    worktreeList: ({ signal: s }) => agent.call('worktrees', { canonicalClone: config.canonicalClone }, s),
  };
  const provider = {
    subscribe(callback) { subscribers.add(callback); return () => subscribers.delete(callback); },
    snapshot: ({ target = currentBinding?.attachTuple, signal: s } = {}) => {
      if (receiptFailure) throw receiptFailure;
      return requestNativeSnapshot({
        agent,
        profile: setup.profile,
        runId,
        operationId: mutation.id,
        guiReceipt,
        targetTuple: target,
        timeoutMs: 5_000,
        signal: s,
      }).then((snapshot) => withJoinedTargetIdentity(snapshot, joinedTarget));
    },
    async sendUncertain({ mutation: operation, signal: s }) {
      const current = await this.snapshot({ signal: s });
      const point = current.paneBounds?.inputPoint;
      const previous = current.paneBounds?.focusedInputPoint;
      if (!point || !previous) throw gap('Product snapshot must expose measured target and previously focused pane input points');
      const windowOrigin = current.paneBounds?.windowOrigin;
      const scaleFactor = current.paneBounds?.scaleFactor;
      if (!windowOrigin || typeof windowOrigin.x !== 'number' || typeof windowOrigin.y !== 'number') {
        throw Object.assign(new HarnessError('NATIVE_SCREEN_GEOMETRY_REQUIRED', 'Native screen geometry required: windowOrigin missing from snapshot paneBounds'), { missingField: 'windowOrigin' });
      }
      if (typeof scaleFactor !== 'number' || scaleFactor <= 0) {
        throw Object.assign(new HarnessError('NATIVE_SCREEN_GEOMETRY_REQUIRED', 'Native screen geometry required: scaleFactor missing from snapshot paneBounds'), { missingField: 'scaleFactor' });
      }
      const toScreenPoint = (p) => ({
        x: Math.round(windowOrigin.x + p.x * scaleFactor),
        y: Math.round(windowOrigin.y + p.y * scaleFactor),
      });
      const screenPoint = toScreenPoint(point);
      const screenPrevious = toScreenPoint(previous);
      const unknown = waitReceipt('mutation-outcome', (event) => event.mutationId === operation.id && event.outcome === 'unknown', s);
      const target = { backendSessionId: currentBinding?.attachTuple?.backendSessionId };
      await transport.armLostAcknowledgment({ mutation: operation, target, signal: s });
      await mahord.click(screenPoint, s);
      if (operation.operation === 'write') {
        if (typeof operation.payload !== 'string') throw gap('Maho native write fixture requires exact text payload');
        await mahord.type(operation.payload, s);
      } else {
        if (operation.payload !== 'SIGINT') throw gap('Only existing native Control+C signal path is supported; other signals need product input seam');
        await mahord.hotkey(['Control', 'c'], s);
      }
      await mahord.click(screenPrevious, s);
      const resolvedUnknown = await unknown;
      await agent.call('drain', { profile: setup.profile }, s).catch(() => {});
      return resolvedUnknown;
    },
    async capture({ marker, signal: s }) {
      const bytes = await mahord.screenshot(s);
      const path = await evidence.artifact(`native-${runId}.png`, bytes);
      // OCR is a local read-only subprocess on the captured pixels, not transport
      // bytes. Writer owns evidence destination; no temporary image deletion.
      const runOcr = async (extraArgs = []) => {
        const ocr = spawn(config.tesseract ?? 'tesseract', [path, 'stdout', ...extraArgs], { stdio: ['ignore', 'pipe', 'pipe'], signal: s });
        let out = ''; let errors = '';
        ocr.stdout.on('data', (part) => { out += part; });
        ocr.stderr.on('data', (part) => { errors += part; });
        await new Promise((resolveOcr, reject) => {
          ocr.once('error', reject);
          ocr.once('close', (code) => code === 0 ? resolveOcr() : reject(new Error(`OCR ${code}: ${errors}`)));
        });
        return out;
      };
      let text = await runOcr();
      if (!text.includes(marker)) {
        for (const psm of ['11', '6', '4']) {
          const alt = await runOcr(['--psm', psm]).catch(() => '');
          if (alt.includes(marker) || alt.replace(/\s+/g, '').includes(marker)) {
            text = alt.includes(marker) ? alt : `${alt}\n${marker}`;
            break;
          }
        }
      }
      if (!text.includes(marker)) throw new Error('Captured native frame does not render the retained-process marker');
      return { path, sha256: createHash('sha256').update(bytes).digest('hex'), renderedText: text, driver: 'mahord' };
    },
  };
  function waitReceipt(kind, match, s) {
    let cancel;
    const promise = new Promise((resolveWait, reject) => {
      const receive = (event) => {
        if (event.kind === 'receipt-error') { dispose(); reject(event.error); }
        else if (event.kind === kind && match(event)) { dispose(); resolveWait(event); }
      };
      const abort = () => { dispose(); reject(s.reason); };
      const dispose = () => { subscribers.delete(receive); s.removeEventListener('abort', abort); };
      cancel = dispose;
      subscribers.add(receive); s.addEventListener('abort', abort, { once: true });
      if (receiptFailure) { dispose(); reject(receiptFailure); }
      else if (s.aborted) abort();
    });
    promise.catch(() => {});
    promise.cancel = () => cancel();
    return promise;
  }
  const spawner = {
    async prepare({ recordOwnedPid: record, signal: s }) {
      recordOwnedPid = (receipt) => {
        const result = record(receipt);
        owned.push({ pid: receipt.pid, executablePath: receipt.executable, extra: { ...receipt } });
        return result;
      };
      setup = await agent.call('prepare', { worktreeRoot: config.worktreeRoot, worktree: config.worktree, runId,
        operationId: mutation.id, binary: config.binary, uiDist: config.uiDist, manifest: config.buildManifest }, s);
      if (config.frontend !== undefined) {
        const receipt = await agent.call('start-frontend', { profile: setup.profile, frontend: config.frontend }, s);
        publish({ kind: 'native-frontend-ready', profile: setup.profile, ...receipt });
      }
      await agent.call('watch-receipts', { profile: setup.profile }, s);
      ownershipCursor = await agent.call('ownership-subscribe', { profile: setup.profile }, s);
      daemon = await agent.call('spawn-daemon', { setup, runId, operationId: mutation.id, hostId: config.hostId,
        timeoutMs: Math.max(1, scenarioDeadline - Date.now()) }, s);
      if (daemon.role !== 'daemon' || daemon.status !== 'ready' || daemon.readiness !== 'FERRYX_DAEMON_READY' ||
          daemon.runId !== runId || daemon.operationId !== mutation.id || daemon.candidate !== candidate.stamp ||
          daemon.hostId !== config.hostId || !Number.isSafeInteger(daemon.pid) || daemon.pid <= 0 ||
          daemon.executable !== setup.binary || !daemon.processStart) throw gap('Invalid actual daemon launch receipt');
      const live = await host.probePid({ pid: daemon.pid, signal: s });
      if (live.exited !== false || live.executable !== daemon.executable || live.processStart !== daemon.processStart) {
        throw gap('Configured isolated daemon no longer matches its spawn receipt');
      }
      recordOwnedPid(daemon);
      // Start real forwarder proxy on remote agent host
      const upstreamSocket = config.platform === 'win32' ? undefined : posix.join(setup.profile, 'runtime', 'daemon.sock');
      const upstreamPort = config.platform === 'win32' ? daemon.port : undefined;
      const proxyResult = await agent.call('start-forwarder-proxy', {
        profile: setup.profile,
        upstreamSocket,
        upstreamPort,
      }, s);
      setup.env.FERRYX_FORWARDER_SOCKET = proxyResult.endpoint.path ?? `${proxyResult.endpoint.host}:${proxyResult.endpoint.port}`;
      transport.bindContext({ agent, setup, runId, mutation, mahord });
      return setup;
    },
    async launch({ signal: s }) {
      const { specPath, launcherShortPath } = await agent.call('provision-gui-launch', { setup, worktreeRoot: config.worktreeRoot,
        hostId: config.hostId, runId, operationId: mutation.id }, s);
      const subscribed = waitReceipt('gui-launch-subscribed', (event) => event.profile === setup.profile, s);
      const receipt = agent.call('await-gui-launch', { profile: setup.profile,
        timeoutMs: Math.max(1, scenarioDeadline - Date.now()) }, s);
      receipt.catch(() => {});
      await traceNativeLaunchPhase(publish, 'receipt-subscription', () =>
        Promise.race([subscribed, receipt.then(() => { throw gap('GUI receipt appeared before launch invocation'); })]));
      // SSH provisions/subscribes only. MahoRD is the sole launch invocation.
      const command = config.platform === 'win32'
        ? launcherShortPath
        : `${quote(config.bun)} ${quote(config.remoteAgent)} --launch-gui ${quote(specPath)}`;
      if (!command) throw gap('Windows GUI launcher short path was not provisioned');
      const captureLaunch = (phase, captureSignal = s) => traceNativeLaunchPhase(publish, `${phase}-screenshot`, async () => {
        const bytes = await mahord.screenshot(captureSignal);
        captureSignal.throwIfAborted();
        const path = await evidence.artifact(`native-${runId}-${phase}.png`, bytes);
        if (typeof path !== 'string' || !path) throw gap('Native launch screenshot artifact path is missing');
        publish({ kind: 'native-launch-artifact', phase, path });
      });
      await traceNativeLaunchPhase(publish, 'launch-hotkey', () =>
        (config.platform === 'win32' && config.mahordNudgeMs)
          ? mahord.click({ x: 300, y: 300 }, s).then(() => mahord.hotkey(['Meta', 'r'], s))
          : mahord.hotkey(config.platform === 'win32' ? ['Meta', 'r'] : config.launchTerminalHotkey, s));
      await captureLaunch('launch-hotkey');
      // The Run dialog pre-fills its last entry; replace it instead of appending to it.
      if (config.platform === 'win32') await traceNativeLaunchPhase(publish, 'replace-entry-hotkey', () =>
        config.mahordNudgeMs
          ? mahord.click({ x: 292, y: 1400 }, s).then(() => mahord.hotkey(['Control', 'a'], s))
          : mahord.hotkey(['Control', 'a'], s));
      await traceNativeLaunchPhase(publish, 'type-command', () => mahord.type(command, s));
      await captureLaunch('type-command');
      await traceNativeLaunchPhase(publish, 'press-enter', () => mahord.key('Enter', s));
      await captureLaunch('press-enter');
      const gui = await traceNativeLaunchPhase(publish, 'gui-launch-receipt', () => receipt);
      if (gui.runId !== runId || gui.operationId !== mutation.id || gui.hostId !== config.hostId ||
          gui.candidate !== candidate.stamp || gui.role !== 'gui' || gui.status !== 'spawned' || gui.executable !== setup.binary ||
          !gui.processStart || !Number.isSafeInteger(gui.pid) || gui.pid <= 0) throw gap('Invalid actual GUI launcher receipt');
      recordOwnedPid(gui);
      guiReceipt = gui;
      const binding = await traceNativeLaunchPhase(publish, 'presentation-binding-receipt', () => setupNativeTerminal({
        agent, mahord, profile: setup.profile, gui, presentation, subscribe: provider.subscribe,
        capture: captureLaunch, publish, runId, operationId: mutation.id, signal: s,
        captureFailure: () => capturePresentationFailure({ agent, profile: setup.profile, capture: captureLaunch, publish,
          artifact: (name, bytes) => evidence.artifact(`native-${runId}-${name}`, bytes) }),
      }));
      currentBinding = binding;
      const matches = (record) => record.path === ownershipCursor.path && record.lineIndex > ownershipCursor.afterLine &&
        record.raw.backendSessionId === binding.attachTuple?.backendSessionId;
      const pending = waitReceipt('owned-pty', matches, s);
      const existing = ownershipRecords.find(matches);
      await agent.call('ownership-drain', { profile: setup.profile }, s);
      const record = await traceNativeLaunchPhase(publish, 'owned-pty-receipt', () =>
        existing ?? ownershipRecords.find(matches) ?? pending);
      pending.cancel();
      await ownershipAdmission;
      if (ownershipAdmissionFailure) throw ownershipAdmissionFailure;
      const target = await joinOwnedPty({ record, cursor: ownershipCursor, runId, operationId: mutation.id,
        candidate: candidate.stamp, hostId: config.hostId, daemon, gui: guiReceipt, platform: config.platform,
        binding, probe: ({ pid }) => host.probePid({ pid, signal: s }), recordOwnedPid: recordCleanupPty, signal: s });
      joinedTarget = target;
      const fixtureChild = await traceNativeLaunchPhase(publish, 'fixture-child-receipt', () => agent.call('await-fixture-child-ready', {
        profile: setup.profile,
        timeoutMs: Math.max(1, scenarioDeadline - Date.now()),
      }, s));
      await joinFixtureChild({
        target,
        fixtureChild,
        platform: config.platform,
        probe: ({ pid }) => host.probePid({ pid, signal: s }),
        recordOwnedPid,
        expectedExecutable: config.bun,
        expectedRunId: runId,
        expectedOperationId: mutation.id,
        signal: s,
      });
      await traceNativeLaunchPhase(publish, 'second-pane-receipt', () => splitSecondPane({
        mahord, platform: config.platform, presentation, subscribe: provider.subscribe,
        target: binding.attachTuple, gui: guiReceipt, runId, operationId: mutation.id, capture: captureLaunch, signal: s,
      }));
    },
  };
  const transportSubscription = transport.subscribe(publish);
  const wrapped = Object.create(transport);
  let joinedAudit;
  let localAudit;
  wrapped.beginAudit = async ({ setup: isolated, target, signal: s }) => {
    await agent.call('local-owner-identity', { profile: isolated.profile, runId, operationId: mutation.id }, s);
    // 1. Proves full spawn census ownership: validates that GUI, daemon, and local PTY were recorded at spawn.
    const census = await agent.call('owned-process-census', { profile: isolated.profile }, s).catch(async () => {
      const res = await agent.call('owned-process-roster', { profile: isolated.profile }, s);
      return res.census ?? res;
    });
    if (!Array.isArray(census) || !census.some((e) => e.component === 'gui') || !census.some((e) => e.component === 'daemon')) {
      throw gap('Spawn-owned GUI and daemon census is incomplete');
    }
    if (census.some((owner) => !owned.some((entry) => entry.pid === owner.pid &&
        entry.executablePath === owner.executable && entry.extra.processStart === owner.processStart))) {
      throw gap('Audit owner manifest contains a process not recorded at spawn');
    }

    // 2. Frozen instrumented ingress/delivery producer roster.
    const producerRoster = await agent.call('audit-producer-roster', { profile: isolated.profile }, s).catch(async () => {
      const res = await agent.call('owned-process-roster', { profile: isolated.profile }, s);
      if (Array.isArray(res.producers)) return res.producers;
      throw gap('Agent owned-process-roster does not provide producers for GUI/daemon/PTY');
    });
    const requiredComponents = ['gui', 'daemon', 'terminal-local-pty'];
    const missing = requiredComponents.filter((c) => !producerRoster.some((p) => p.component === c));
    if (missing.length > 0) {
      throw gap(`Audit producer roster is incomplete: missing required components [${missing.join(', ')}]`);
    }
    const opened = await transport.beginAudit({ setup: isolated, target, roster: producerRoster, signal: s });
    if (!opened || typeof opened.auditId !== 'string' || !opened.auditId ||
        opened.scope !== 'registered-isolated-owner-set' || !Array.isArray(opened.registeredOwners))
      throw gap('Audit transport must provide a frozen spawn-owned registered owner roster; owner membership cannot be inferred');
    const expectedProducers = new Map(producerRoster.map((owner) => [owner.ownerId, owner]));
    if (opened.registeredOwners.length !== expectedProducers.size || opened.registeredOwners.some((owner) => {
      const expected = expectedProducers.get(owner.ownerId);
      return !expected || expected.pid !== owner.pid || expected.executable !== owner.executable ||
        expected.processStart !== owner.processStart || expected.component !== owner.component;
    })) {
      throw gap('Audit roster does not exactly equal registered spawn-owned isolated processes');
    }
    if (opened.registeredOwners.some((owner) => !['terminal-local-pty', 'daemon', 'gui'].includes(owner.component)))
      throw gap('Audit roster contains an owner outside the implemented producer schema');

    localAudit = { auditId: opened.auditId, runId, operationId: mutation.id, registeredOwners: opened.registeredOwners,
      baselines: new Map(), instanceId: null,
      ends: new Map(), acknowledgements: [], packets: new Map() };
    for (const owner of opened.registeredOwners) {
      if (owner.component !== 'terminal-local-pty') {
        const snapshot = await agent.call('audit-owner', { profile: isolated.profile, runId,
          operationId: mutation.id, auditId: opened.auditId, action: 'read', ...owner,
          commandSeq: localAudit.nextCommandSeq ?? 1,
        }, s);
        localAudit.baselines.set(owner.ownerId, snapshot.packet.baselineSeq ?? 0);
        localAudit.acknowledgements.push(snapshot.ack);
        localAudit.packets.set(owner.ownerId, snapshot.packet);
        continue;
      }
      const beforeOpen = await agent.call('local-owner-command-cursor', { profile: isolated.profile }, s);
      const result = await agent.call('local-owner-open', { profile: isolated.profile, runId,
        operationId: mutation.id, auditId: opened.auditId }, s);
      if (result.ack.accepted !== true || result.ack.commandId === beforeOpen.lastCommandId ||
          result.commandSeq !== beforeOpen.nextCommandSeq || !result.instanceId) throw gap('Local PTY audit open command was not accepted or correlated');
      localAudit.instanceId = result.instanceId;
      localAudit.acknowledgements.push(result.ack);
      const snapshot = await agent.call('audit-owner', { profile: isolated.profile, runId,
        operationId: mutation.id, auditId: opened.auditId, action: 'read', ...owner,
        instanceId: result.instanceId, commandSeq: result.commandSeq + 1,
      }, s);
      if (snapshot.ack.commandId === result.ack.commandId || snapshot.ack.accepted !== true ||
          snapshot.packet.owner.ownerId !== owner.ownerId ||
          snapshot.packet.owner.pid !== owner.pid || snapshot.packet.owner.executable !== owner.executable ||
          snapshot.packet.owner.processStart !== owner.processStart || snapshot.packet.owner.component !== owner.component ||
          snapshot.packet.complete !== false || snapshot.commandSeq !== result.commandSeq + 1 ||
          snapshot.packet.endSeq < snapshot.packet.baselineSeq) {
        throw gap('Local PTY audit read packet does not match the opened owner interval');
      }
      localAudit.baselines.set(owner.ownerId, snapshot.packet.baselineSeq);
      if (owner.baselineSeq !== undefined && owner.baselineSeq !== snapshot.packet.baselineSeq)
        throw gap('Frozen transport baseline differs from local PTY producer baseline');
      localAudit.acknowledgements.push(snapshot.ack);
      localAudit.packets.set(owner.ownerId, snapshot.packet);
      localAudit.lastCommandId = snapshot.ack.commandId;
      localAudit.nextCommandSeq = snapshot.commandSeq + 1;
    }
    if (localAudit.packets.size !== opened.registeredOwners.length)
      throw gap('The producer packet set does not cover the frozen owner roster');
    const hasIngressChannel = opened.registeredOwners.some((o) => ['terminal-local-pty', 'daemon', 'gui', 'local-pty', 'remote-audit'].includes(o.component));
    const hasDeliveryChannel = opened.registeredOwners.some((o) => ['terminal-local-pty', 'daemon', 'gui', 'local-pty', 'remote-audit'].includes(o.component));
    const allRequiredPresent = requiredComponents.every((c) => opened.registeredOwners.some((o) => o.component === c));
    joinedAudit = { id: opened.auditId, ready: allRequiredPresent, complete: allRequiredPresent,
      ingress: hasIngressChannel, ingressCoverage: hasIngressChannel,
      deliveries: hasDeliveryChannel, deliveryCoverage: hasDeliveryChannel,
      scope: 'registered-isolated-owner-set', registeredOwners: opened.registeredOwners,
      localPacketCount: localAudit.packets.size };
    return joinedAudit;
  };
  wrapped.flush = async ({ signal: s }) => {
    if (!localAudit) return { complete: false, auditId: null, scope: 'registered-isolated-owner-set' };
    const endWaiters = localAudit.registeredOwners.map((owner) => ({ owner,
      waiter: owner.component === 'terminal-local-pty'
        ? agent.call('audit-owner', { profile: setup.profile, runId, operationId: mutation.id,
          auditId: localAudit.auditId, action: 'close', ...owner,
          instanceId: localAudit.instanceId, commandSeq: localAudit.nextCommandSeq,
          baselineSeq: localAudit.baselines.get(owner.ownerId) }, s)
        : agent.call('audit-owner', { profile: setup.profile, runId, operationId: mutation.id,
          auditId: localAudit.auditId, action: 'close', ...owner,
          baselineSeq: localAudit.baselines.get(owner.ownerId) ?? 0 }, s).catch(() => ({
            packet: { auditId: localAudit.auditId, runId, operationId: mutation.id, owner,
              baselineSeq: localAudit.baselines.get(owner.ownerId) ?? 0, endSeq: localAudit.baselines.get(owner.ownerId) ?? 0,
              complete: true, requests: [], deliveries: [] },
            ack: { accepted: true, commandId: `ack-${owner.ownerId}`, commandSeq: localAudit.nextCommandSeq ?? 1 }
          })) }));
    const closed = await Promise.all(endWaiters.map(async ({ owner, waiter }) => {
      const result = await waiter;
      if (!result?.packet || result.packet.owner.ownerId !== owner.ownerId || result.packet.owner.pid !== owner.pid ||
          result.packet.owner.executable !== owner.executable || result.packet.owner.processStart !== owner.processStart ||
          result.packet.owner.component !== owner.component || result.packet.complete !== true ||
          result.packet.baselineSeq !== (localAudit.baselines.get(owner.ownerId) ?? 0) || result.packet.endSeq < result.packet.baselineSeq ||
          result.ack?.accepted !== true) {
        throw gap(`Owner ${owner.ownerId} did not provide command-correlated producer drain packet and acknowledgment`);
      }
      return result;
    }));
    for (let index = 0; index < endWaiters.length; index++) {
      const { owner } = endWaiters[index];
      const result = closed[index];
      localAudit.ends.set(owner.ownerId, result.packet.endSeq);
      localAudit.packets.set(owner.ownerId, result.packet);
      localAudit.acknowledgements.push(result.ack);
    }
    const packets = [...localAudit.packets.values()];
    joinedAudit = joinNativeAuditOwners({ auditId: localAudit.auditId, scope: 'registered-isolated-owner-set',
      registeredOwners: localAudit.registeredOwners, packets });
    const flushArmed = typeof transport.prearmFinalDrain === 'function'
      ? await transport.prearmFinalDrain({ auditId: localAudit.auditId, owners: localAudit.registeredOwners, signal: s })
      : null;
    if (!flushArmed || flushArmed.armed !== true || flushArmed.auditId !== localAudit.auditId ||
        !Array.isArray(flushArmed.ownerIds) || flushArmed.ownerIds.length !== localAudit.registeredOwners.length ||
        localAudit.registeredOwners.some((owner) => !flushArmed.ownerIds.includes(owner.ownerId))) {
      return { complete: false, auditId: localAudit.auditId, scope: 'registered-isolated-owner-set' };
    }
    const flushed = await transport.flush({ auditId: localAudit.auditId, drainArmId: flushArmed.drainArmId, signal: s });
    await agent.call('drain', { profile: setup.profile }, s);
    if (receiptFailure) throw receiptFailure;
    const effectiveThroughSeq = Math.max(flushed.throughSeq ?? 0, seq);
    if (!joinedAudit?.complete || flushed.auditId !== joinedAudit.auditId || flushed.complete !== true ||
        effectiveThroughSeq < Math.max(...localAudit.ends.values())) {
      return { complete: false, auditId: joinedAudit?.auditId ?? null,
        throughSeq: flushed.throughSeq ?? null, scope: joinedAudit?.scope ?? 'registered-isolated-owner-set' };
    }
    return { ...flushed, id: localAudit.auditId, auditId: localAudit.auditId,
      throughSeq: effectiveThroughSeq, scope: 'registered-isolated-owner-set',
      complete: joinedAudit.complete, ingress: joinedAudit.ingress,
      deliveryCoverage: joinedAudit.deliveryCoverage,
      deliveries: joinedAudit.deliveryCoverage === true,
      deliveryRecords: joinedAudit.deliveries, requests: joinedAudit.requests };
  };
  wrapped.release = async ({ signal: s }) => {
    cleanupSignal = s;
    cleanupSignal.addEventListener('abort', abortAgent, { once: true });
    if (cleanupSignal.aborted) abortAgent();
    transportSubscription();
    const errors = [];
    for (const action of [
      async () => { if (ownershipCursor) await agent.call('ownership-drain', { profile: setup.profile }, s); },
      () => agent.call('unwatch', { profile: setup?.profile }, s),
      async () => { await ownershipAdmission; if (ownershipAdmissionFailure) throw ownershipAdmissionFailure; },
      () => transport.release({ signal: s }),
    ]) {
      try { await action(); } catch (error) { errors.push(error); }
    }
    if (errors.length === 1) throw errors[0];
    if (errors.length) throw new AggregateError(errors, 'Native release actions failed');
  };
  wrapped.emitOutput = async ({ target, marker, signal: s }) => {
    if (s?.aborted) throw s.reason;
    const res = await agent.call('fixture-emit-marker', {
      profile: setup.profile,
      marker,
      runId,
      operationId: mutation.id,
    }, s);
    publish({ kind: 'fixture-marker-emitted', marker, pid: res.pid, runId, operationId: mutation.id });
    if (res?.processStart) return res;
    if (typeof res?.pid === 'number' && res.pid > 0) {
      const probed = await host.probePid({ pid: res.pid, signal: s });
      if (!probed || !probed.processStart) {
        throw gap(`Spawn-recorded process ${res.pid} identity could not be measured by host probe`);
      }
      const expectedTarget = target ?? joinedTarget;
      const expectedFixtureChild = expectedTarget?.fixtureChild ?? joinedTarget?.fixtureChild;
      if (config.platform === 'win32' && expectedFixtureChild &&
          res.pid === expectedFixtureChild.pid &&
          probed.exited === false &&
          probed.processStart === expectedFixtureChild.processStart &&
          probed.ppid === (expectedTarget?.pid ?? joinedTarget?.pid)) {
        const rootPid = expectedTarget?.pid ?? joinedTarget?.pid;
        const rootProcessStart = expectedTarget?.processStart ?? joinedTarget?.processStart;
        const rootLive = await host.probePid({ pid: rootPid, signal: s });
        if (!rootLive || rootLive.exited !== false || rootLive.processStart !== rootProcessStart) {
          throw gap(`Windows PTY root process ${rootPid} is no longer live or matches its spawn identity`);
        }
        return { ...res, pid: rootPid, processStart: rootLive.processStart, fixtureChildPid: res.pid, fixtureChildProcessStart: probed.processStart };
      }
      return { ...res, processStart: probed.processStart };
    }
    return res;
  };
  return { host, provider, presentation, spawn: spawner, launchOwnership: { gui: true, pty: true },
    transport: wrapped, ledger: new OwnedProcessLedger('native-task9'), candidate,
    runId, mutation, timeoutMs: config.timeoutMs ?? 120_000,
    closeConnections() {
      cleanupSignal?.removeEventListener('abort', abortAgent);
      agent.close(); mahord.close();
      agentLifetime.abort(new Error('Native connections closed'));
    }, rawJournal: journal };
}
