/**
 * Regression coverage for the daemon control transport (defect 19: the harness dialled
 * `<runtime>/daemon.sock` on Windows, where the product publishes `daemon.port` +
 * `daemon.token` instead, so every provisioning run died with a bare ENOENT).
 *
 * Static and pure assertions: no socket is opened, no daemon is started, nothing is written,
 * and the only host query is a read-only process-table probe (the same one the launcher's
 * identity check uses) with a PID that cannot exist. The platform is injected, so the Windows
 * branch is exercised on every host rather than only where the defect was found.
 */

import assert from 'node:assert/strict';
import test, { after } from 'node:test';
import { createServer } from 'node:net';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import {
  DAEMON_CONTROL_PROTOCOL_VERSION,
  DAEMON_LOOPBACK_HOST,
  DAEMON_TRANSPORT_FILES,
  DAEMON_TRANSPORT_KINDS,
  ISOLATED_GATEWAY_OWNERSHIP_CONTRACT,
  IsolatedGatewayError,
  REFERENCE_CREDENTIAL_TIERS,
  REFERENCE_HOST_ACCESS_KINDS,
  REFERENCE_NONLOCAL_TRANSPORTS,
  adoptOwnedGateway,
  connectDaemonControl,
  daemonControlTransport,
  daemonHandshakeFrame,
  daemonRequestFailure,
  daemonSocketPath,
  describeDaemonRequestFailure,
  transportErrorCode,
  executablePathMatches,
  hostAccessContract,
  normalizeExecutablePath,
  ownerHostReceiptRequirement,
  parseDaemonPortFile,
  parseDaemonTransportToken,
} from './herdr-reference-fixtures.mjs';

const root = resolve('herdr-transport-fixture');
const runtime = join(root, 'runtime');
const profile = { root, paths: { runtime } };

test('a unix profile publishes a socket and no token', () => {
  for (const platform of ['linux', 'darwin']) {
    const transport = daemonControlTransport(profile, { platform });
    assert.equal(transport.kind, DAEMON_TRANSPORT_KINDS.unixSocket, platform);
    assert.equal(transport.platform, platform);
    assert.equal(transport.socketPath, join(runtime, DAEMON_TRANSPORT_FILES.unixSocket));
    assert.equal(transport.tokenFile, null, 'unix has no token file at all');
    assert.equal(transport.requiresToken, false);
    assert.equal(transport.portFile, undefined, 'unix never publishes a port file');
  }
});

test('a win32 profile publishes the loopback port and token pair, never a socket', () => {
  const transport = daemonControlTransport(profile, { platform: 'win32' });
  assert.equal(transport.kind, DAEMON_TRANSPORT_KINDS.loopbackTcp);
  assert.equal(transport.host, DAEMON_LOOPBACK_HOST);
  assert.equal(transport.portFile, join(runtime, DAEMON_TRANSPORT_FILES.port));
  assert.equal(transport.tokenFile, join(runtime, DAEMON_TRANSPORT_FILES.token));
  assert.equal(transport.requiresToken, true);
  // The defect itself: this branch used to be byte-identical to the unix one, so the
  // harness dialled a file the product never creates on Windows.
  assert.equal(transport.socketPath, undefined);
  assert.ok(!JSON.stringify(transport).includes(DAEMON_TRANSPORT_FILES.unixSocket));
  assert.notDeepEqual(daemonControlTransport(profile, { platform: 'win32' }), daemonControlTransport(profile, { platform: 'linux' }));
});

test('the default platform selects the branch this host publishes', () => {
  const transport = daemonControlTransport(profile);
  assert.equal(transport.kind, process.platform === 'win32' ? DAEMON_TRANSPORT_KINDS.loopbackTcp : DAEMON_TRANSPORT_KINDS.unixSocket);
});

test('a runtime directory outside the profile is refused outright', () => {
  const foreign = { root, paths: { runtime: join(root, '..', 'not-this-profile') } };
  assert.throws(
    () => daemonControlTransport(foreign, { platform: 'win32' }),
    (error) => {
      assert.ok(error instanceof IsolatedGatewayError, 'the launcher maps this type to BLOCKED');
      assert.equal(error.reason, 'daemon-runtime-not-profile-owned');
      return true;
    },
  );
});

test('a published token is a credential only when it has content', () => {
  for (const absent of [undefined, null, '', '   ', '\n', '\t\r\n']) {
    assert.equal(parseDaemonTransportToken(absent), null, JSON.stringify(absent));
  }
  assert.equal(parseDaemonTransportToken(' 9Zq3r8Tf2kLp\n'), '9Zq3r8Tf2kLp');
  assert.equal(parseDaemonTransportToken('9Zq3r8Tf2kLp'), '9Zq3r8Tf2kLp');
});

test('a port file is read as exactly one decimal in range', () => {
  assert.equal(parseDaemonPortFile('43821'), 43821);
  assert.equal(parseDaemonPortFile(' 41234\n'), 41234);
  assert.equal(parseDaemonPortFile('1'), 1);
  assert.equal(parseDaemonPortFile('65535'), 65535);
  for (const bad of [undefined, null, '', '   ', '0', '65536', '-1', '41234abc', '0x1f', '1e3', '41234.0', '123456']) {
    assert.equal(parseDaemonPortFile(bad), null, JSON.stringify(bad));
  }
});

test('the handshake frame carries the token only where the transport requires it', () => {
  const unix = daemonHandshakeFrame(daemonControlTransport(profile, { platform: 'linux' }), null);
  const tcp = daemonHandshakeFrame(daemonControlTransport(profile, { platform: 'win32' }), 'secret-token');
  for (const frame of [unix, tcp]) {
    assert.equal(frame.endsWith('\n'), true, 'the daemon reads one line per frame');
    assert.equal(frame.slice(0, -1).includes('\n'), false, 'exactly one line');
  }
  const unixPayload = JSON.parse(unix);
  const tcpPayload = JSON.parse(tcp);
  assert.equal(unixPayload.type, 'handshake');
  assert.equal(unixPayload.version, DAEMON_CONTROL_PROTOCOL_VERSION);
  assert.equal(Object.hasOwn(unixPayload, 'token'), false, 'unix presents no credential');
  assert.equal(tcpPayload.token, 'secret-token', 'the first frame must carry this boot token');
  assert.equal(tcpPayload.version, DAEMON_CONTROL_PROTOCOL_VERSION);
});

test('daemonSocketPath stays the unix transport only', () => {
  assert.equal(daemonSocketPath(profile), join(runtime, DAEMON_TRANSPORT_FILES.unixSocket));
});

// ---- ownership handoff ------------------------------------------------------
// These cover the refusals that must happen BEFORE any process is signalled or any socket is
// opened, which is what makes them assertable without a daemon. `adoptOwnedGateway` returns a
// rejected promise for each, and nothing in them reaches the network.

const ownedRecord = (overrides) => ({
  contract: ISOLATED_GATEWAY_OWNERSHIP_CONTRACT,
  profileRoot: join(root, 'profile'),
  runtimeDir: runtime,
  platform: 'win32',
  transportKind: DAEMON_TRANSPORT_KINDS.loopbackTcp,
  daemonPid: 4242,
  daemonExecutablePath: join(root, 'ferryx.exe'),
  daemonReportedPid: 4242,
  daemonEpoch: '7',
  url: 'http://127.0.0.1:43821',
  credentialFile: join(runtime, 'reference-chat-token'),
  lease: { ownerPid: 1, createdAt: 0, deadlineAt: Date.now() + 60_000 },
  ...(overrides || {}),
});

// Every refusal below is a caller that IS authorized, so the reason under test is the record's
// and not the missing flag (which has its own test).
const refusal = async (value, reason) => {
  await assert.rejects(
    () => adoptOwnedGateway(value, { timeoutMs: 1000, allowHost: true }),
    (error) => {
      assert.ok(error instanceof IsolatedGatewayError, 'a refusal must be the BLOCKED type');
      assert.equal(error.reason, reason);
      return true;
    },
  );
};

test('an ownership record from a different contract is refused', async () => {
  await refusal({ ...ownedRecord(), contract: 'something-else/1' }, 'owned-gateway-record-invalid');
  await refusal(null, 'owned-gateway-record-invalid');
  await refusal('not-a-record', 'owned-gateway-record-invalid');
});

test('an incomplete ownership record is refused before anything is dialled', async () => {
  for (const field of ['profileRoot', 'runtimeDir', 'platform', 'daemonPid', 'daemonExecutablePath', 'url', 'credentialFile']) {
    await refusal(ownedRecord({ [field]: null }), 'owned-gateway-record-incomplete');
  }
});

test('an unbounded or expired lease is refused', async () => {
  await refusal(ownedRecord({ lease: null }), 'owned-gateway-lease-missing');
  await refusal(ownedRecord({ lease: { ownerPid: 1 } }), 'owned-gateway-lease-missing');
  await refusal(
    ownedRecord({ lease: { ownerPid: 1, createdAt: 0, deadlineAt: Date.now() - 1 } }),
    'owned-gateway-lease-expired',
  );
});

test('a proven-dead recorded PID is refused rather than signalled', async () => {
  // 4294967294 is outside the kernel's PID range on every supported platform, so the identity
  // probe reports it dead: adoption must stop there, never fall through to a signal.
  await refusal(ownedRecord({ daemonPid: 4294967294 }), 'owned-gateway-not-running');
});

// ---- identity is a PATH, never a name ---------------------------------------
// A fixture manifest is data, so an ownership record can name any path. These pin that the
// comparison is full normalized path equality: a shared basename in another directory must
// never read as the same executable.

test('a basename in a different directory is never the same executable', () => {
  const live = join(root, 'other-dir', 'ferryx.exe');
  const recorded = join(root, 'recorded-dir', 'ferryx.exe');
  for (const platform of ['win32', 'linux', 'darwin']) {
    assert.equal(executablePathMatches(live, recorded, platform), false, platform);
    assert.equal(normalizeExecutablePath(live, platform) === normalizeExecutablePath(recorded, platform), false, platform);
  }
  // The case that used to pass: a live path that merely ENDS WITH the recorded basename.
  assert.equal(executablePathMatches('/opt/ferryx/ferryx.exe', join(root, 'stage', 'ferryx.exe'), 'linux'), false);
});

test('the same file spelled differently is the same executable on win32 only', () => {
  const canonical = 'C:\\Stage\\Frozen\\ferryx.exe';
  const forward = 'c:/stage/frozen/ferryx.exe';
  const trailing = 'C:\\Stage\\Frozen\\ferryx.exe\\';
  assert.equal(executablePathMatches(forward, canonical, 'win32'), true, 'separators and case fold on win32');
  assert.equal(executablePathMatches(trailing, canonical, 'win32'), true, 'a trailing separator is not a different file');
  assert.equal(executablePathMatches(canonical, canonical, 'win32'), true);
  // POSIX filesystems are case-sensitive: the same spelling rule must NOT be applied there.
  assert.equal(executablePathMatches('/Stage/frozen/ferryx', '/stage/frozen/ferryx', 'linux'), false);
  assert.equal(executablePathMatches('/stage/frozen/ferryx', '/stage/frozen/ferryx', 'linux'), true);
});

test('an unreadable identity is never a match', () => {
  const recorded = join(root, 'stage', 'ferryx.exe');
  for (const live of [null, undefined, '', '   ']) {
    assert.equal(executablePathMatches(live, recorded, 'win32'), false, JSON.stringify(live));
  }
  assert.equal(executablePathMatches(recorded, null, 'win32'), false);
  assert.equal(normalizeExecutablePath(null, 'win32'), null);
  assert.equal(normalizeExecutablePath('   ', 'win32'), null);
});

// ---- non-local access contract -------------------------------------------------
// A non-local host is reached over the frozen HTTP routes at its own gateway, so the fixture
// must declare that endpoint and credential. These pin that an incomplete fixture is reported
// as the exact missing piece - never as the transport being unsupported.

const nonLocalHost = (overrides) => ({
  id: 'qa-ssh-1',
  transport: 'ssh',
  url: 'http://192.0.2.10:43821',
  credentialFile: join(root, 'creds', 'ssh-token'),
  ...(overrides || {}),
});

// The credential read is injected so no host file is needed: the contract under test is the
// endpoint/tier derivation, not the filesystem.
const readableCredential = { ok: true, tier: REFERENCE_CREDENTIAL_TIERS.machine, token: 'qa-token', path: join(root, 'creds', 'ssh-token') };

test('a non-local host is an HTTP gateway, never a daemon transport', () => {
  for (const transport of REFERENCE_NONLOCAL_TRANSPORTS) {
    const access = hostAccessContract(nonLocalHost({ transport }), { credential: readableCredential });
    assert.equal(access.kind, REFERENCE_HOST_ACCESS_KINDS.httpGateway, transport);
    assert.equal(access.providedBy, 'provisioned-host');
    assert.equal(access.url, 'http://192.0.2.10:43821', 'the trailing slash is normalized away');
  }
  // A local host's url/credential come from the daemon this run launches or adopts.
  const local = hostAccessContract({ id: 'qa-local', transport: 'local' });
  assert.equal(local.kind, REFERENCE_HOST_ACCESS_KINDS.localDaemon);
  assert.equal(local.providedBy, 'isolated-launch');
  assert.equal(local.url, null);
});

test('an unconfigured non-local endpoint or credential is named exactly', () => {
  const cases = [
    [nonLocalHost({ url: '' }), 'host-url-missing'],
    [nonLocalHost({ url: undefined }), 'host-url-missing'],
    [nonLocalHost({ url: '192.0.2.10:43821' }), 'host-url-not-http'],
    [nonLocalHost({ credentialFile: '' }), 'host-credential-missing'],
    [nonLocalHost({ credentialFile: join(root, 'absent-token') }), 'host-credential-unreadable'],
    [nonLocalHost({ transport: 'carrier-pigeon' }), 'host-transport-unknown'],
  ];
  for (const [host, reason] of cases) {
    assert.throws(
      () => hostAccessContract(host),
      (error) => {
        assert.ok(error instanceof IsolatedGatewayError, reason);
        assert.equal(error.reason, reason);
        return true;
      },
    );
  }
});

test('a receipt requirement is named for a non-local session without one', () => {
  const requirement = ownerHostReceiptRequirement(nonLocalHost(), { backendSessionId: 'sess-1' });
  assert.ok(requirement, 'an unbound non-local session must carry a requirement');
  assert.equal(requirement.kind, 'owner-host-spawn-receipt');
  assert.equal(requirement.backendSessionId, 'sess-1');
  assert.ok(requirement.requiredFields.includes('epoch'));
  assert.ok(requirement.requiredFields.includes('candidate'));
  // A session that already carries one, and any local session, need nothing further.
  assert.equal(ownerHostReceiptRequirement(nonLocalHost(), { backendSessionId: 's', spawnReceiptPath: '/tmp/r.json' }), null);
  assert.equal(ownerHostReceiptRequirement({ id: 'qa-local', transport: 'local' }, { backendSessionId: 's' }), null);
});

test('adoption requires explicit authorization, and checks it before the record', async () => {
  await assert.rejects(
    () => adoptOwnedGateway(ownedRecord(), { timeoutMs: 1000 }),
    (error) => {
      assert.ok(error instanceof IsolatedGatewayError);
      assert.equal(error.reason, 'owned-gateway-authorization-required');
      return true;
    },
  );
  await assert.rejects(
    () => adoptOwnedGateway(ownedRecord(), { timeoutMs: 1000, allowHost: false }),
    (error) => {
      assert.equal(error.reason, 'owned-gateway-authorization-required');
      return true;
    },
  );
  // A malformed record with no authorization reports the AUTHORIZATION, not the record: a
  // caller must not be able to probe a fixture-supplied record without the flag.
  await assert.rejects(
    () => adoptOwnedGateway(null, { timeoutMs: 1000 }),
    (error) => {
      assert.equal(error.reason, 'owned-gateway-authorization-required');
      return true;
    },
  );
});

// ---- live control-transport behaviour ---------------------------------------
// These drive the REAL client against a loopback listener this file owns, because the defect
// they cover is an unhandled stream event: a handler that is missing does not fail an
// assertion, it kills the process with "Unhandled 'error' event ... on Interface instance".
// Passing these tests is itself the assertion that both the socket and the readline Interface
// are subscribed.
//
// No sleep and no polling anywhere: every step waits on the event it needs (the server's
// listener, the client's own close notification, or the settled promise).

const liveTransports = [];
const startFakeDaemon = async (onConnection) => {
  const dir = mkdtempSync(join(tmpdir(), 'herdr-transport-test-'));
  const runtime = join(dir, 'runtime');
  mkdirSync(runtime, { recursive: true });
  const server = createServer(onConnection);
  await new Promise((resolveListening) => server.listen(0, '127.0.0.1', resolveListening));
  const port = server.address().port;
  writeFileSync(join(runtime, 'daemon.port'), String(port));
  writeFileSync(join(runtime, 'daemon.token'), 'qa-transport-token');
  // The Windows descriptor is chosen deliberately: it is the transport the reset was reported
  // on, and it exercises the token read on every host.
  const transport = daemonControlTransport({ root: dir, paths: { runtime } }, { platform: 'win32' });
  const handle = {
    transport,
    dir,
    port,
    server,
    close() {
      return new Promise((resolveClosed) => server.close(resolveClosed));
    },
    cleanup() {
      rmSync(dir, { recursive: true, force: true });
    },
  };
  liveTransports.push(handle);
  return handle;
};

after(() => {
  for (const handle of liveTransports) handle.cleanup();
});

/** Serve the handshake, then hand the connection to `afterHandshake`. */
const handshakeThen = (afterHandshake) => (socket) => {
  let buffered = '';
  let shook = false;
  socket.on('data', (chunk) => {
    buffered += chunk.toString();
    const lines = buffered.split('\n');
    buffered = lines.pop();
    for (const line of lines) {
      if (line.trim().length === 0) continue;
      if (!shook) {
        shook = true;
        socket.write(JSON.stringify({ type: 'handshakeOk', version: 5, pid: 4242, epoch: 7 }) + '\n');
        continue;
      }
      afterHandshake(socket, line);
    }
  });
};

test('a reset while a request is in flight settles it with a structured failure', async () => {
  const daemon = await startFakeDaemon(
    handshakeThen((socket) => {
      // Force a real RST: a plain destroy() closes without an error, which would test the
      // close path instead of the reset path this defect is about.
      if (typeof socket.resetAndDestroy === 'function') socket.resetAndDestroy();
      else socket.destroy(new Error('reset'));
    }),
  );
  const client = await connectDaemonControl(daemon.transport, { timeoutMs: 5000 });
  const response = await client.call({ type: 'remoteGetStatus' });
  const failure = daemonRequestFailure(response);
  assert.ok(failure, 'a transport that ends mid-request must settle with a structured failure, not null');
  assert.ok(
    ['daemon-transport-reset', 'daemon-transport-closed'].includes(failure.code),
    'unexpected code: ' + failure.code,
  );
  assert.equal(failure.requestKind, 'remoteGetStatus', 'the failure names the request kind');
  assert.ok(typeof failure.message === 'string' && failure.message.length > 0);
  // One settlement, however many stream events fired: the trail records the first cause and
  // marks the rest as ignored.
  const trail = client.diagnostics();
  assert.equal(trail.settled, true);
  assert.equal(trail.events.filter((entry) => entry.event === 'settled').length, 1);
  client.close();
});

test('once the connection has ended, a request with nothing pending is refused structurally', async () => {
  const daemon = await startFakeDaemon(
    handshakeThen((socket) => {
      if (typeof socket.resetAndDestroy === 'function') socket.resetAndDestroy();
      else socket.destroy(new Error('reset'));
    }),
  );
  const client = await connectDaemonControl(daemon.transport, { timeoutMs: 5000 });
  // The close notification is registered while the connection is still healthy, so it cannot
  // be missed: the server only ends the socket once it receives the probe below.
  const closed = new Promise((resolveClosed) => client.onClose(resolveClosed));
  const probe = await client.call({ type: 'remoteGetStatus' });
  assert.ok(daemonRequestFailure(probe), 'the probe must settle as a failure');
  await closed;
  // Nothing is pending now. This is the idle case: the request must be refused immediately
  // with a structured failure, and it must not throw, hang, or resolve null.
  const idle = await client.call({ type: 'remoteGetStatus' });
  const failure = daemonRequestFailure(idle);
  assert.ok(failure, 'a request on a dead connection must not hang or resolve null');
  assert.equal(failure.code, 'daemon-transport-closed');
  assert.equal(failure.requestKind, 'remoteGetStatus');
  assert.equal(client.diagnostics().events.filter((entry) => entry.event === 'settled').length, 1);
  client.close();
});

test('close and reset are distinguishable, and neither settles twice', async () => {
  const resetDaemon = await startFakeDaemon(
    handshakeThen((socket) => {
      if (typeof socket.resetAndDestroy === 'function') socket.resetAndDestroy();
      else socket.destroy(new Error('reset'));
    }),
  );
  const resetClient = await connectDaemonControl(resetDaemon.transport, { timeoutMs: 5000 });
  const resetResponse = await resetClient.call({ type: 'remoteGetStatus' });
  const resetFailure = daemonRequestFailure(resetResponse);
  assert.ok(resetFailure);
  // The trail must show at most one settlement and must have observed a close as well: the
  // two events arrive in either order and only the first decides the outcome.
  const resetTrail = resetClient.diagnostics();
  assert.equal(resetTrail.events.filter((entry) => entry.event === 'settled').length, 1);
  assert.ok(resetTrail.events.some((entry) => entry.event === 'socket-close'));
  assert.ok(
    resetTrail.events.some((entry) => entry.event === 'socket-error' || entry.event === 'interface-error'),
    'a reset must be observed as a stream error, not only as a close',
  );
  resetClient.close();

  // A clean close with a request in flight is the other outcome: the same shape, the closed
  // code, and still exactly one settlement.
  const closeDaemon = await startFakeDaemon(
    handshakeThen((socket) => socket.end()),
  );
  const closeClient = await connectDaemonControl(closeDaemon.transport, { timeoutMs: 5000 });
  const closeResponse = await closeClient.call({ type: 'remoteGetStatus' });
  const closeFailure = daemonRequestFailure(closeResponse);
  assert.ok(closeFailure, 'a clean close must settle in flight requests too');
  assert.equal(closeFailure.code, 'daemon-transport-closed');
  assert.equal(closeClient.diagnostics().events.filter((entry) => entry.event === 'settled').length, 1);
  closeClient.close();
});

test('the failure detail names the request without leaking the transport token', async () => {
  const daemon = await startFakeDaemon(
    handshakeThen((socket) => {
      if (typeof socket.resetAndDestroy === 'function') socket.resetAndDestroy();
      else socket.destroy(new Error('reset'));
    }),
  );
  const client = await connectDaemonControl(daemon.transport, { timeoutMs: 5000 });
  const response = await client.call({ type: 'spawn', clientRequestId: 'qa-1' });
  const failure = daemonRequestFailure(response);
  const described = describeDaemonRequestFailure(failure);
  assert.ok(described.includes(failure.code));
  assert.ok(described.includes('spawn'), 'the description names the request kind');
  const trail = JSON.stringify(client.diagnostics());
  assert.equal(trail.includes('qa-transport-token'), false, 'diagnostics must not carry the token');
  client.close();
});

test('a stream error maps to a typed code from the errno, never from the message', () => {
  assert.equal(transportErrorCode({ code: 'ECONNRESET' }), 'daemon-transport-reset');
  assert.equal(transportErrorCode({ code: 'ECONNREFUSED' }), 'daemon-transport-refused');
  assert.equal(transportErrorCode({ code: 'EPIPE' }), 'daemon-transport-pipe-closed');
  assert.equal(transportErrorCode({ code: 'ENOENT' }), 'daemon-transport-missing');
  assert.equal(transportErrorCode({ code: 'ETIMEDOUT' }), 'daemon-transport-timed-out');
  assert.equal(transportErrorCode({ code: 'EWHATEVER' }), 'daemon-transport-error');
  assert.equal(transportErrorCode(new Error('read ECONNRESET')), 'daemon-transport-error');
  assert.equal(transportErrorCode(null), 'daemon-transport-error');
});

