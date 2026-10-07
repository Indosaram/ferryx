/**
 * Regression coverage for the daemon control transport (defect 19: the harness dialled
 * `<runtime>/daemon.sock` on Windows, where the product publishes `daemon.port` +
 * `daemon.token` instead, so every provisioning run died with a bare ENOENT).
 *
 * Mostly static and pure assertions. A handful drive the REAL client against a loopback listener
 * this file owns, because the behaviour they cover is a stream event: a missing handler does not
 * fail an assertion, it kills the process. Every listener and client a test starts is closed by
 * that test or by the hook below, so the suite can exit on its own. The platform is injected, so
 * the Windows branch is exercised on every host rather than only where the defect was found.
 */

import assert from 'node:assert/strict';
import test, { after } from 'node:test';
import { createServer } from 'node:net';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve, sep } from 'node:path';
import {
  DAEMON_CONTROL_PROTOCOL_VERSION,
  DAEMON_LOOPBACK_HOST,
  DIAGNOSTIC_BYTE_LIMIT,
  DIAGNOSTIC_CAPTURE_TIMEOUT_MS,
  DAEMON_TRANSPORT_FILES,
  DAEMON_TRANSPORT_KINDS,
  ISOLATED_GATEWAY_OWNERSHIP_CONTRACT,
  IsolatedGatewayError,
  ReferenceAuthorityError,
  REFERENCE_CREDENTIAL_TIERS,
  REFERENCE_HOST_ACCESS_KINDS,
  REFERENCE_NONLOCAL_TRANSPORTS,
  FIXTURE_SCHEMA,
  adoptOwnedGateway,
  boundedBytesReport,
  captureSessionDiagnostics,
  connectDaemonControl,
  daemonControlTransport,
  daemonHandshakeFrame,
  daemonRequestFailure,
  daemonSocketPath,
  deadline,
  describeDaemonRequestFailure,
  executablePathMatches,
  hostAccessContract,
  normalizeExecutablePath,
  ownerHostReceiptRequirement,
  parseDaemonPortFile,
  parseDaemonTransportToken,
  referenceReadQuery,
  registerWorkspaceOnDaemon,
  resolveReferenceAuthority,
  targetFor,
  targetForRead,
  transportErrorCode,
  validateFixtureManifest,
  validateReferenceCapabilities,
  workspaceIdRefusal,
  workspaceRegistrationFor,
} from './herdr-reference-fixtures.mjs';
import {
  HOST_SUPPLIED_SOURCE_KIND,
  validateOwnerHostSpawnReceipt,
} from './herdr-reference-pty-identity.mjs';
import {
  ProvisionError,
  SSH_RECEIPT_PROBE_PLATFORM,
  SSH_HOST_STORE_FILENAME,
  SSH_PROJECTS_STORE_FILENAME,
  remoteSpawnRequest,
  remoteExecutableProbeCommand,
  remoteOwnerHostReceipt,
  remoteReceiptCorrelation,
  sshProjectsStorePath,
  sshConnectionArgs,
} from './herdr-reference-provision.mjs';

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
const liveClients = [];

/** Connect a tracked client, so a test that throws mid-way still gets its socket closed. */
const openClient = async (transport) => {
  const client = await connectDaemonControl(transport, { timeoutMs: 5000 });
  liveClients.push(client);
  return client;
};

const startFakeDaemon = async (onConnection) => {
  const dir = mkdtempSync(join(tmpdir(), 'herdr-transport-test-'));
  const runtime = join(dir, 'runtime');
  mkdirSync(runtime, { recursive: true });
  // The sockets this fixture accepts are owned by the fixture and tracked as exact objects.
  // net.Server has no closeAllConnections - that is an http.Server API - so teardown destroys
  // precisely these sockets and only then awaits server.close(). Nothing here calls a method
  // that may not exist, and nothing waits on a connection this fixture does not own.
  const accepted = new Set();
  const server = createServer((socket) => {
    accepted.add(socket);
    socket.once('close', () => accepted.delete(socket));
    onConnection(socket);
  });
  await new Promise((resolveListening) => server.listen(0, '127.0.0.1', resolveListening));
  const port = server.address().port;
  writeFileSync(join(runtime, 'daemon.port'), String(port));
  writeFileSync(join(runtime, 'daemon.token'), 'qa-transport-token');
  // The Windows descriptor is chosen deliberately: it is the transport the reset was reported
  // on, and it exercises the token read on every host.
  const transport = daemonControlTransport({ root: dir, paths: { runtime } }, { platform: 'win32' });
  let closed = false;
  const handle = {
    transport,
    dir,
    port,
    server,
    get closed() {
      return closed;
    },
    // A listening server is a live handle: it holds the event loop open, so a test that never
    // closes one leaves node with nothing to do and no reason to exit - which is exactly how
    // this file printed its outcomes and then sat idle instead of reporting. Closing is
    // awaited, and it is idempotent so a second close is not an error.
    async close() {
      if (closed) return;
      closed = true;
      // Destroy the exact sockets this server accepted, then close the listener. A plain
      // close() waits for existing connections, so without this the await below would block on
      // a connection no test owns - the hang this fixture exists to avoid. Destroying an
      // already-ended socket is harmless, which is why this is unconditional.
      for (const socket of accepted) socket.destroy();
      accepted.clear();
      await new Promise((resolveClosed) => server.close(resolveClosed));
    },
    cleanup() {
      rmSync(dir, { recursive: true, force: true });
    },
  };
  liveTransports.push(handle);
  return handle;
};

// The safety net is awaited and it CLOSES both owned resources, not just their directories:
// removing a temp dir leaves the listener running and the process alive, and an assertion that
// throws mid-test skips the test's own cleanup. That second case is why this hook closes
// clients too - a client socket holds the event loop open exactly like a listener does.
//
// It also reports, in one bounded line, which handles a test left open. That is evidence for the
// next merged run: a suite that prints its outcomes and then sits idle with no summary has an
// unclosed handle, and this line names it instead of leaving the cause to be guessed.
after(async () => {
  const openServers = liveTransports.filter((handle) => !handle.closed).map((handle) => handle.port);
  const openClients = liveClients.filter((client) => !client.closed).map((client) => client.endpoint);
  for (const client of liveClients) {
    try {
      client.close();
    } catch {
      /* A client whose socket already died has nothing to close. */
    }
  }
  for (const handle of liveTransports) {
    try {
      await handle.close();
    } catch {
      /* A server that never listened has nothing to close. */
    }
    handle.cleanup();
  }
  if (openServers.length > 0 || openClients.length > 0) {
    // Bounded on purpose: counts and endpoints, never a full handle dump.
    process.stdout.write(
      "# owned handles left open by tests: servers=" + openServers.length +
        " [" + openServers.join(",") + "] clients=" + openClients.length +
        " [" + openClients.join(",") + "] (closed by this hook)\n",
    );
  }
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
  const client = await openClient(daemon.transport);
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
  await daemon.close();
});

test('once the connection has ended, a request with nothing pending is refused structurally', async () => {
  const daemon = await startFakeDaemon(
    handshakeThen((socket) => {
      if (typeof socket.resetAndDestroy === 'function') socket.resetAndDestroy();
      else socket.destroy(new Error('reset'));
    }),
  );
  const client = await openClient(daemon.transport);
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
  await daemon.close();
});

// The DISTINCTION under test is: a reset surfaces as a stream error, a graceful end never does.
// What is deliberately NOT asserted is the ORDER in which the error and the close arrive: after
// a reset, Node may deliver close on a later tick than the error that settles the request, so an
// assertion that close has already happened at settle time is an ordering claim the platform
// does not make (it failed on Windows AND on macOS for exactly that reason). The close is
// asserted where it is observable - awaited, with a bound - and the code is asserted to follow
// whichever cause was observed FIRST, which is the contract this client actually implements.
test('a reset and a graceful close are distinguishable, and neither settles twice', async () => {
  const resetDaemon = await startFakeDaemon(
    handshakeThen((socket) => {
      // Force a real RST: a plain destroy() closes without an error, which would exercise the
      // close path instead of the reset path this test is about.
      if (typeof socket.resetAndDestroy === 'function') socket.resetAndDestroy();
      else socket.destroy(new Error('reset'));
    }),
  );
  const resetClient = await openClient(resetDaemon.transport);
  // Armed BEFORE the trigger, so the close cannot be missed however late it arrives.
  const resetClosed = new Promise((resolveClosed) => resetClient.onClose(resolveClosed));
  const resetResponse = await resetClient.call({ type: 'remoteGetStatus' });
  const resetFailure = daemonRequestFailure(resetResponse);
  assert.ok(resetFailure, 'a reset must settle the in-flight request');
  const resetTrail = resetClient.diagnostics();
  const firstError = resetTrail.events.findIndex(
    (entry) => entry.event === 'socket-error' || entry.event === 'interface-error',
  );
  const firstClose = resetTrail.events.findIndex((entry) => entry.event === 'socket-close');
  assert.notEqual(firstError, -1, 'a reset must be observed as a stream error, not only as a close');
  // The first observed cause decides the code - the same rule the client implements, stated in
  // terms that hold whichever order the platform chose.
  const expectedCode =
    firstClose === -1 || firstError < firstClose ? 'daemon-transport-reset' : 'daemon-transport-closed';
  assert.equal(resetFailure.code, expectedCode, 'the first observed cause decides the code');
  assert.equal(resetTrail.events.filter((entry) => entry.event === 'settled').length, 1);
  // The close itself, where it is actually observable: awaited and bounded, never assumed.
  await deadline(resetClosed, 'the reset connection must close', 5000);
  assert.ok(
    resetClient.diagnostics().events.some((entry) => entry.event === 'socket-close'),
    'the reset connection must have observed its close',
  );
  assert.equal(
    resetClient.diagnostics().events.filter((entry) => entry.event === 'settled').length,
    1,
    'a close that arrives after the error must not settle the request a second time',
  );
  resetClient.close();
  await resetDaemon.close();

  // A graceful end is the other outcome: the same structured failure, the closed code, exactly
  // one settlement - and NO stream error, which is what makes the two distinguishable.
  const closeDaemon = await startFakeDaemon(
    handshakeThen((socket) => socket.end()),
  );
  const closeClient = await openClient(closeDaemon.transport);
  const closeResponse = await closeClient.call({ type: 'remoteGetStatus' });
  const closeFailure = daemonRequestFailure(closeResponse);
  assert.ok(closeFailure, 'a clean close must settle in flight requests too');
  assert.equal(closeFailure.code, 'daemon-transport-closed');
  assert.equal(
    closeClient.diagnostics().events.some(
      (entry) => entry.event === 'socket-error' || entry.event === 'interface-error',
    ),
    false,
    'a graceful end must NOT surface as a stream error; that absence is the discriminator',
  );
  assert.equal(closeClient.diagnostics().events.filter((entry) => entry.event === 'settled').length, 1);
  closeClient.close();
  await closeDaemon.close();
});

test('the failure detail names the request without leaking the transport token', async () => {
  const daemon = await startFakeDaemon(
    handshakeThen((socket) => {
      if (typeof socket.resetAndDestroy === 'function') socket.resetAndDestroy();
      else socket.destroy(new Error('reset'));
    }),
  );
  const client = await openClient(daemon.transport);
  const response = await client.call({ type: 'spawn', clientRequestId: 'qa-1' });
  const failure = daemonRequestFailure(response);
  const described = describeDaemonRequestFailure(failure);
  assert.ok(described.includes(failure.code));
  assert.ok(described.includes('spawn'), 'the description names the request kind');
  const trail = JSON.stringify(client.diagnostics());
  assert.equal(trail.includes('qa-transport-token'), false, 'diagnostics must not carry the token');
  client.close();
  await daemon.close();
});

// ---- workspace registration contract -----------------------------------------
// A daemon refuses a spawn for a workspace nothing registered, and its own startup registration
// derives an id from the launch cwd - which is why the config's id must be registered explicitly.
// These pin the frame, the id rules the daemon enforces, and the exact refusal when the config
// cannot supply what registration needs.

test('the workspace id rules mirror the registry, and name why an id is refused', () => {
  for (const good of ['prov-d51e2085', 'ws-123', 'a', 'ws_1.2']) {
    assert.equal(workspaceIdRefusal(good), null, good);
  }
  assert.ok(workspaceIdRefusal(''), 'an empty id must be refused');
  assert.ok(workspaceIdRefusal('   '), 'a whitespace-only id must be refused');
  assert.ok(workspaceIdRefusal(undefined), 'a missing id must be refused');
  assert.ok(workspaceIdRefusal(null));
  assert.ok(workspaceIdRefusal('-leading-dash'));
  assert.ok(workspaceIdRefusal('has/slash'));
  assert.ok(workspaceIdRefusal('has\\backslash'));
  assert.ok(workspaceIdRefusal('has space'));
  assert.ok(workspaceIdRefusal('has\ttab'));
  assert.ok(workspaceIdRefusal('daemon:ws'), 'the daemon namespace is refused by the local registry');
  assert.ok(workspaceIdRefusal('remote:host/ws'));
});

test('a registration names the config root explicitly, never an inferred one', () => {
  const root = mkdtempSync(join(tmpdir(), 'herdr-ws-root-'));
  try {
    const ok = workspaceRegistrationFor({ id: 'h' }, { workspaceId: 'prov-d51e2085', repoRoot: root });
    assert.equal(ok.ok, true);
    assert.equal(ok.workspaceId, 'prov-d51e2085');
    // The frame is the daemon's own wire shape: camelCase fields under a camelCase type tag.
    assert.deepEqual(ok.request, {
      type: 'registerWorkspace',
      workspaceId: 'prov-d51e2085',
      repoRoot: ok.repoRoot,
    });
    assert.equal(Object.hasOwn(ok.request, 'workspace_id'), false, 'the wire uses camelCase');

    // A host-level root is accepted as the fallback, and the id is trimmed onto the wire.
    const hostRoot = workspaceRegistrationFor({ id: 'h', repoRoot: root }, { workspaceId: '  ws-1  ' });
    assert.equal(hostRoot.ok, true);
    assert.equal(hostRoot.workspaceId, 'ws-1');
    assert.equal(hostRoot.request.workspaceId, 'ws-1');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('a config that cannot supply a usable root is refused by field, not by daemon error', () => {
  const missing = workspaceRegistrationFor({ id: 'h' }, { workspaceId: 'ws-1' });
  assert.equal(missing.ok, false);
  assert.equal(missing.reason, 'workspace-repo-root-missing');

  const relative = workspaceRegistrationFor({ id: 'h' }, { workspaceId: 'ws-1', repoRoot: 'relative/dir' });
  assert.equal(relative.ok, false);
  assert.equal(relative.reason, 'workspace-repo-root-not-absolute');

  const absent = workspaceRegistrationFor({ id: 'h' }, { workspaceId: 'ws-1', repoRoot: join(root, 'absent-root') });
  assert.equal(absent.ok, false);
  assert.equal(absent.reason, 'workspace-repo-root-absent');

  // The id is checked first: an unusable id is reported even when the root is also missing.
  const badId = workspaceRegistrationFor({ id: 'h' }, { workspaceId: '', repoRoot: join(root, 'absent-root') });
  assert.equal(badId.reason, 'workspace-id-invalid');
});

test('the registrar sends the frame and reports the daemon reply', async () => {
  const seen = [];
  const daemon = await startFakeDaemon(
    handshakeThen((socket, line) => {
      seen.push(JSON.parse(line));
      socket.write(JSON.stringify({ type: 'registerWorkspaceOk' }) + '\n');
    }),
  );
  const client = await openClient(daemon.transport);
  const registration = workspaceRegistrationFor({ id: 'h' }, { workspaceId: 'prov-d51e2085', repoRoot: root });
  assert.equal(registration.ok, false, 'this root does not exist, so the fixture needs a real one');
  const realRoot = mkdtempSync(join(tmpdir(), 'herdr-ws-root-'));
  try {
    const usable = workspaceRegistrationFor({ id: 'h' }, { workspaceId: 'prov-d51e2085', repoRoot: realRoot });
    assert.equal(usable.ok, true);
    const result = await registerWorkspaceOnDaemon(client, usable, { requestId: 'prov-d51e2085' });
    assert.deepEqual(result, { ok: true, workspaceId: 'prov-d51e2085', repoRoot: usable.repoRoot });
    assert.equal(seen.length, 1, 'exactly one registration frame');
    assert.equal(seen[0].type, 'registerWorkspace');
    assert.equal(seen[0].workspaceId, 'prov-d51e2085');
    assert.equal(typeof seen[0].repoRoot, 'string');
  } finally {
    rmSync(realRoot, { recursive: true, force: true });
  }
  client.close();
  await daemon.close();
});

test('a refused registration is reported with the daemon message, not as a success', async () => {
  const daemon = await startFakeDaemon(
    handshakeThen((socket) => {
      socket.write(
        JSON.stringify({ type: 'error', message: "Workspace 'prov-d51e2085' is not registered" }) + '\n',
      );
    }),
  );
  const client = await openClient(daemon.transport);
  const realRoot = mkdtempSync(join(tmpdir(), 'herdr-ws-root-'));
  try {
    const usable = workspaceRegistrationFor({ id: 'h' }, { workspaceId: 'prov-d51e2085', repoRoot: realRoot });
    const result = await registerWorkspaceOnDaemon(client, usable, {});
    assert.equal(result.ok, false);
    assert.ok(result.detail.includes('not registered'), 'the daemon message is carried through');
    assert.ok(result.detail.includes('registerWorkspace'), 'the failure names the request kind');
  } finally {
    rmSync(realRoot, { recursive: true, force: true });
  }
  client.close();
  await daemon.close();
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

// ---- fixture manifest contract (producer -> validator -> consumer) -----------
// ONE defect, and one NON-defect that must not be "fixed" by loosening a check.
//
// The defect: the producer writes a pid as a NUMBER (`record.ptyChild.pid`) while the validator
// tested it with a non-empty-STRING predicate, so it reported "session is missing pid" for a pid
// that was present. That is the contract mismatch these tests pin.
//
// The non-defect: a local host's url/credentialFile being null was NOT a validator bug. The
// provisioner publishes those fields for a local host exactly when it RETAINED the gateway
// (`--retain-gateway`), which is what keeps the same daemon and sessions across the two runs; a
// stopped launch is recorded as evidence only because its url would be dead. A null there means the
// run did not retain, so the check that refuses it is correct and must stay strict.

const manifestFor = (host, session) => ({
  schema: 'ferryx-herdr-reference.fixtures/1',
  // A fresh host object per manifest, so no test can couple to another through shared state.
  hosts: [{ ...host }],
  sessions: [{
    hostId: host.id,
    backendSessionId: 'sess-1',
    executablePath: '/opt/ferryx/ferryx',
    pid: 3211174,
    provider: 'omo',
    ...(session || {}),
  }],
  devices: [],
  spawnLedger: { schema: 'ferryx-herdr-reference.spawn-ledger/1', entries: [] },
});

// A valid local host now declares the retained access: url + credentialFile. The credential path
// is RELATIVE on purpose - the validator's existence check applies to absolute paths only, so a
// relative one keeps these fixtures hermetic (no filesystem dependency, no temp dir to clean up).
const LOCAL_HOST = {
  id: 'local-win',
  transport: 'local',
  url: 'http://127.0.0.1:43821',
  credentialFile: 'creds/local-token',
  startLocal: true,
};

test('a numeric pid is accepted, and only a positive integer counts as one', () => {
  const ok = validateFixtureManifest(manifestFor(LOCAL_HOST));
  assert.equal(ok.ok, true, JSON.stringify(ok.errors));
  // The producer writes this shape; requiring a string was the defect.
  assert.equal(ok.errors.includes('session is missing pid'), false);
  for (const [pid, why] of [
    ['3211174', 'a string pid is not a pid'],
    [0, 'zero is not a pid'],
    [-1, 'a negative pid is not a pid'],
    [12.5, 'a fractional pid is not a pid'],
    [Number.NaN, 'NaN is not a pid'],
    [Number.POSITIVE_INFINITY, 'infinity is not a pid'],
    [undefined, 'an absent pid is missing'],
  ]) {
    const verdict = validateFixtureManifest(manifestFor(LOCAL_HOST, { pid }));
    assert.equal(verdict.ok, false, why);
    assert.ok(
      verdict.errors.some((e) => e.includes('pid')),
      why + ' -> ' + JSON.stringify(verdict.errors),
    );
  }
});

test('a local host must declare the retained gateway access, and a dead or null url is refused', () => {
  // The producer publishes url + credentialFile for a local host exactly when it RETAINED the
  // gateway it launched - that handoff is what keeps the same daemon and sessions across the two
  // runs. A launch it stopped at exit is evidence only, because its url is dead by the time the
  // runner reads it. So null here means "not retained", and it must be refused, not waived.
  const retained = validateFixtureManifest(manifestFor({
    id: 'local-win',
    transport: 'local',
    url: 'http://127.0.0.1:43821',
    credentialFile: 'creds/local-token',
    ownedGateway: { contract: 'ferryx-herdr-reference.isolated-gateway-ownership/1' },
  }));
  assert.equal(retained.ok, true, JSON.stringify(retained.errors));

  for (const [host, why] of [
    [{ id: 'local-win', transport: 'local', url: null, credentialFile: null }, 'neither field'],
    [{ id: 'local-win', transport: 'local', url: null, credentialFile: 'creds/t' }, 'null url'],
    [{ id: 'local-win', transport: 'local', url: 'http://127.0.0.1:43821', credentialFile: null }, 'null credential'],
    [{ id: 'local-win', transport: 'local', url: '', credentialFile: '' }, 'empty strings'],
    // A retained gateway whose credential file is gone is a dead access path: the manifest names
    // it, and the validator refuses a credential it cannot read.
    [{ id: 'local-win', transport: 'local', url: 'http://127.0.0.1:43821', credentialFile: join(root, 'absent-token') }, 'absent credential file'],
  ]) {
    const verdict = validateFixtureManifest(manifestFor(host));
    assert.equal(verdict.ok, false, why);
    assert.ok(
      verdict.errors.some((e) => e.includes('url') || e.includes('credential')),
      why + ' -> ' + JSON.stringify(verdict.errors),
    );
  }
});

test('a non-local host still must declare its endpoint and credential', () => {
  for (const transport of REFERENCE_NONLOCAL_TRANSPORTS) {
    const bare = validateFixtureManifest(manifestFor({ id: 'h-' + transport, transport, url: null, credentialFile: null }));
    assert.equal(bare.ok, false, transport);
    assert.ok(bare.errors.some((e) => e.includes('url')), transport + ' -> ' + JSON.stringify(bare.errors));
    assert.ok(bare.errors.some((e) => e.includes('credentialFile')), transport);
    const declared = validateFixtureManifest(manifestFor({
      id: 'h-' + transport,
      transport,
      url: 'http://192.0.2.10:43821',
      credentialFile: 'creds/token',
    }));
    assert.equal(declared.ok, true, transport + ' -> ' + JSON.stringify(declared.errors));
  }
});

test('a provider is required and is never defaulted by the validator', () => {
  const missing = validateFixtureManifest(manifestFor(LOCAL_HOST, { provider: undefined }));
  assert.equal(missing.ok, false);
  assert.ok(missing.errors.includes('session is missing provider'), JSON.stringify(missing.errors));
  const empty = validateFixtureManifest(manifestFor(LOCAL_HOST, { provider: '   ' }));
  assert.equal(empty.ok, false);
  assert.ok(empty.errors.some((e) => e.includes('provider is not a non-empty string')));
  // The validator reports the absence; it does not substitute a provider of its own.
  const ok = validateFixtureManifest(manifestFor(LOCAL_HOST, { provider: 'codex' }));
  assert.equal(ok.ok, true, JSON.stringify(ok.errors));
  assert.equal(ok.sessions[0].provider, 'codex');
});

// ---- diagnostic capture -----------------------------------------------------
// The capture exists to explain a failure without becoming one: it is bounded, it is cancelled by
// closing the connection the run owns, and every encoding of the bytes it keeps is derived from the
// REDACTED form. These reuse the deterministic fake daemon above - a server that completes the
// handshake and then either answers or never does. A never-answering server makes the bound the only
// possible outcome, so there is no sleep and no polling anywhere in these tests.

const SECRET_JSON = '{"token":"s3cr3t-value-1234"}';
const SECRET_BEARER = 'Authorization: Bearer abcdefghijklmnopqrst';

test('a kept byte range is bounded and says so when it truncated', () => {
  const report = boundedBytesReport(Buffer.alloc(DIAGNOSTIC_BYTE_LIMIT + 512, 0x61));
  assert.equal(report.byteLength, DIAGNOSTIC_BYTE_LIMIT + 512);
  assert.equal(report.keptBytes, DIAGNOSTIC_BYTE_LIMIT);
  assert.equal(report.truncated, true);
  assert.equal(Buffer.from(report.base64, 'base64').length, DIAGNOSTIC_BYTE_LIMIT);
  // A capture shorter than the bound is not marked truncated, so a small read is never mistaken
  // for a clipped one - and control bytes stay visible instead of vanishing into invisible text.
  const small = boundedBytesReport(Buffer.from('\x1b[6n', 'utf8'));
  assert.equal(small.truncated, false);
  assert.equal(small.escaped, '\\x1b[6n');
});

test('a secret is absent from the escaped text and from the decoded base64', () => {
  const report = boundedBytesReport(Buffer.from(SECRET_JSON + ' ' + SECRET_BEARER, 'utf8'));
  assert.equal(report.redactionApplied, true);
  assert.equal(report.escaped.includes('s3cr3t-value-1234'), false);
  assert.equal(report.escaped.includes('abcdefghijklmnopqrst'), false);
  const decoded = Buffer.from(report.base64, 'base64').toString('utf8');
  assert.equal(decoded.includes('s3cr3t-value-1234'), false, 'base64 must derive from the redacted form');
  assert.equal(decoded.includes('abcdefghijklmnopqrst'), false);
  // One representation, not two: decoding the field yields exactly what the readable form shows.
  assert.equal(decoded, report.escaped);
  assert.ok(decoded.includes('<REDACTED>'));
});

test('a completed capture reports the daemon details and the escaped replay', async () => {
  const probe = Buffer.from('\x1b[6n', 'utf8');
  const daemon = await startFakeDaemon(
    handshakeThen((socket, line) => {
      const request = JSON.parse(line);
      if (request.type === 'describeSession') {
        socket.write(JSON.stringify({
          type: 'describeSessionOk',
          session: {
            sessionId: 'sess-1',
            workspaceId: 'ws-1',
            running: true,
            suspended: false,
            lastOutputAgeMs: null,
            cols: 80,
            rows: 24,
          },
        }) + '\n');
        return;
      }
      if (request.type === 'attach') {
        socket.write(JSON.stringify({
          type: 'attachOk',
          epoch: 7,
          sessionId: 'sess-1',
          startSequence: 1,
          endSequence: 1,
          history: probe.toString('base64'),
          ptyCols: 80,
          ptyRows: 24,
        }) + '\n');
      }
    }),
  );
  const client = await openClient(daemon.transport);
  const report = await captureSessionDiagnostics(client, 'sess-1');
  assert.equal(report.diagnosticUnavailable, null);
  assert.equal(report.session.running, true);
  assert.equal(report.session.lastOutputAgeMs, null, 'no output observed yet is null, not zero');
  assert.equal(report.session.cols, 80);
  assert.equal(report.attach.epoch, 7);
  assert.equal(report.replay.byteLength, probe.length);
  assert.equal(report.replay.escaped, '\\x1b[6n');
  client.close();
  await daemon.close();
});

test('the bound expires as diagnosticUnavailable, never as an empty session', async () => {
  assert.ok(DIAGNOSTIC_CAPTURE_TIMEOUT_MS > 0 && DIAGNOSTIC_CAPTURE_TIMEOUT_MS <= 10000);
  // The server completes the handshake and then never answers, so the bound is the only outcome.
  const daemon = await startFakeDaemon(handshakeThen(() => {}));
  const client = await openClient(daemon.transport);
  const report = await captureSessionDiagnostics(client, 'sess-1', { timeoutMs: 40 });
  assert.ok(report.diagnosticUnavailable, 'an expired capture must report itself unavailable');
  assert.equal(report.session, null, 'nothing was read, so nothing may be reported as a session');
  assert.equal(report.sessionFailure, null);
  assert.equal(report.replay, null);
  // It resolves. A diagnostic that rejected would displace the failure it was gathered to explain.
  client.close();
  await daemon.close();
});

test('closing the owned connection settles the reads it cancelled', async () => {
  const daemon = await startFakeDaemon(handshakeThen(() => {}));
  const client = await openClient(daemon.transport);
  const pending = client.call({ type: 'describeSession', sessionId: 'sess-1' });
  client.close();
  const settled = await pending;
  const failure = daemonRequestFailure(settled);
  assert.ok(failure, 'a pending read must settle structurally when the owned connection closes');
  assert.equal(failure.code, 'daemon-transport-closed');
  assert.equal(client.closed, true);
  // And a capture issued afterwards still resolves, so the original identity error is retained:
  // the unreadable details are REPORTED, never thrown over the caller's own failure.
  const report = await captureSessionDiagnostics(client, 'sess-1');
  assert.equal(report.session, null);
  assert.ok(report.sessionFailure, 'unreadable details are reported, not raised');
  assert.ok(report.replayFailure);
  await daemon.close();
});

/* ==========================================================================
 * The reference-chat authority the gateway publishes (referenceOwnerId)
 * ==========================================================================
 *
 * The owner a reference read or mutation carries is the one the OWNING gateway published in
 * `/api/v1/capabilities`, beside `referenceHostId` and for the same incarnation as `daemonEpoch`.
 * These are source-only assertions: they bind the published value, and the typed refusal that
 * stands in place of a configured owner, without a gateway and without a runtime.
 */

const publishedCapabilities = {
  daemonEpoch: '41',
  referenceHostId: 'host-published',
  referenceOwnerId: 'owner-published',
  machineId: 'machine-not-a-host-id',
};

function authoritativeSession(overrides) {
  return {
    hostId: 'host-1',
    referenceHostId: 'host-published',
    referenceOwnerId: 'owner-published',
    ownerId: 'owner-published',
    epoch: '41',
    backendSessionId: 'sess-1',
    providerSessionId: 'provider-1',
    registryId: 'codex-cli',
    ...(overrides || {}),
  };
}

test('a read carries the published owner, and omits the host id', () => {
  const params = new URLSearchParams(referenceReadQuery(authoritativeSession(), {}, 'screen'));
  assert.equal(params.get('ownerId'), 'owner-published');
  // A read omits hostId entirely: the gateway resolves its own reference host id.
  assert.equal(params.get('hostId'), null);
});

test('a configured owner that is not the published one is refused, not sent', () => {
  assert.throws(
    () => referenceReadQuery(authoritativeSession({ ownerId: 'owner-from-config' }), {}, 'screen'),
    (error) => error instanceof ReferenceAuthorityError && error.code === 'reference-owner-not-authoritative',
  );
});

test('a row with no published owner never falls back to the configured one', () => {
  assert.throws(
    () => referenceReadQuery(authoritativeSession({ referenceOwnerId: null }), {}, 'screen'),
    (error) => error instanceof ReferenceAuthorityError && error.code === 'reference-owner-absent',
  );
  assert.throws(
    () => targetFor(authoritativeSession({ referenceOwnerId: undefined })),
    (error) => error instanceof ReferenceAuthorityError && error.code === 'reference-owner-absent',
  );
});

test('a mutation target echoes the published owner and the published host id', () => {
  const target = targetFor(authoritativeSession());
  assert.equal(target.hostId, 'host-published');
  assert.equal(target.ownerId, 'owner-published');
  assert.equal(target.epoch, '41');
  assert.equal(target.backendSessionId, 'sess-1');
  // machineId is a different value and is never substituted for the host id.
  assert.notEqual(target.hostId, 'machine-not-a-host-id');
});

test('a read target is the owner and the incarnation, with no host id', () => {
  const target = targetForRead(authoritativeSession());
  assert.deepEqual(Object.keys(target).sort(), ['backendSessionId', 'epoch', 'ownerId']);
  assert.equal(target.ownerId, 'owner-published');
  assert.equal(target.epoch, '41');
});

test('the capability validator requires the owner published beside the host id', () => {
  const ok = validateReferenceCapabilities(publishedCapabilities);
  assert.equal(ok.ok, true);
  assert.equal(ok.referenceHostId, 'host-published');
  assert.equal(ok.referenceOwnerId, 'owner-published');

  const absent = validateReferenceCapabilities({ daemonEpoch: '41', referenceHostId: 'host-published' });
  assert.equal(absent.ok, false);
  assert.deepEqual(absent.missing, ['referenceOwnerId']);
  assert.equal(absent.referenceOwnerId, null);
});

test('authority resolves from the gateway, and a changed owner is an invalidation', () => {
  assert.deepEqual(
    resolveReferenceAuthority(publishedCapabilities, authoritativeSession()),
    { hostId: 'host-published', ownerId: 'owner-published' },
  );
  assert.throws(
    () => resolveReferenceAuthority(publishedCapabilities, authoritativeSession({ ownerId: 'owner-from-config' })),
    (error) => error instanceof ReferenceAuthorityError && error.code === 'reference-owner-changed',
  );
  assert.throws(
    () =>
      resolveReferenceAuthority(
        { daemonEpoch: '41', referenceHostId: 'host-published' },
        authoritativeSession(),
      ),
    (error) => error instanceof ReferenceAuthorityError && error.code === 'reference-authority-absent',
  );
});

test('a record that publishes a host id without its owner is refused', () => {
  const withOwner = {
    schema: FIXTURE_SCHEMA,
    hosts: [
      {
        id: 'host-1',
        transport: 'local',
        url: 'http://127.0.0.1:1',
        credentialFile: '/nonexistent-credential',
        referenceHostId: 'host-published',
        referenceOwnerId: 'owner-published',
      },
    ],
    sessions: [
      {
        hostId: 'host-1',
        backendSessionId: 'sess-1',
        executablePath: '/bin/sh',
        pid: 1,
        provider: 'codex-cli',
        referenceHostId: 'host-published',
        referenceOwnerId: 'owner-published',
      },
    ],
  };
  const complete = validateFixtureManifest(withOwner);
  assert.equal(
    complete.errors.filter((message) => /without referenceOwnerId/.test(message)).length,
    0,
    'a complete published pair must not be reported as incomplete: ' + complete.errors.join('; '),
  );

  const incomplete = validateFixtureManifest({
    ...withOwner,
    hosts: [{ ...withOwner.hosts[0], referenceOwnerId: undefined }],
    sessions: [{ ...withOwner.sessions[0], referenceOwnerId: undefined }],
  });
  const named = incomplete.errors.filter((message) => /without referenceOwnerId/.test(message));
  assert.equal(named.length, 2, 'host and session are each named: ' + incomplete.errors.join('; '));
  assert.equal(incomplete.ok, false);
});

/* ==========================================================================
 * The ssh owner-host receipt, correlated by the owning daemon's own reply
 * ==========================================================================
 *
 * Source-only assertions over the pure path: a `remoteSessionDetails` reply and the declared
 * create record go in, a correlated receipt comes out, and the SHARED validator then accepts it.
 * No transport, no host and no bridge is involved here.
 */

const daemonSessionId = 'daemon-session-1';
const helperSessionId = 'helper-session-1';
const helperTarget = {
  hostId: 'host-1',
  ownerId: 'owner-1',
  epoch: 41,
  backendSessionId: helperSessionId,
};
const createRecord = { sessionId: daemonSessionId, clientRequestId: 'req-1' };
const candidateProvenance = {
  candidateId: 'cand-1',
  sourceManifestSha256: 'a'.repeat(64),
  binarySha256: 'b'.repeat(64),
};

/** The daemon's own reply, as RemoteSessionDetails serializes it. */
function remoteDetails(overrides) {
  return {
    descriptor: {
      backendSessionId: daemonSessionId,
      target: { ...helperTarget },
      clientRequestId: 'req-1',
      cols: 80,
      rows: 24,
    },
    state: 'connected',
    generation: 1,
    attempts: 0,
    failure: null,
    replayGap: null,
    pid: 4242,
    ...(overrides || {}),
  };
}

function correlatedReceipt(overrides) {
  const correlation = remoteReceiptCorrelation(remoteDetails((overrides || {}).details), createRecord);
  assert.equal(correlation.ok, true, 'the fixture must correlate: ' + JSON.stringify(correlation));
  return remoteOwnerHostReceipt({
    correlation,
    hostId: 'host-1',
    epoch: 41,
    remotePlatform: 'linux',
    executable: '/usr/bin/bash',
    observedAt: '2026-10-07T00:00:06.000Z',
    candidate: candidateProvenance,
  });
}

test('the daemon reply correlates its session id with the helper target', () => {
  const correlation = remoteReceiptCorrelation(remoteDetails(), createRecord);
  assert.equal(correlation.ok, true);
  // The two identities stay separate values; neither is relabelled as the other.
  assert.equal(correlation.daemonSessionId, daemonSessionId);
  assert.equal(correlation.helperSessionId, helperSessionId);
  assert.notEqual(correlation.daemonSessionId, correlation.helperSessionId);
  assert.deepEqual(correlation.helperTarget, helperTarget);
  assert.equal(correlation.clientRequestId, 'req-1');
  assert.equal(correlation.pid, 4242);
});

test('a session id or request id the daemon does not confirm is refused', () => {
  const wrongSession = remoteReceiptCorrelation(
    remoteDetails({ descriptor: { ...remoteDetails().descriptor, backendSessionId: 'some-other-session' } }),
    createRecord,
  );
  assert.equal(wrongSession.ok, false);
  assert.equal(wrongSession.reason, 'ssh-correlation-session-mismatch');

  const wrongRequest = remoteReceiptCorrelation(
    remoteDetails({ descriptor: { ...remoteDetails().descriptor, clientRequestId: 'some-other-request' } }),
    createRecord,
  );
  assert.equal(wrongRequest.ok, false);
  assert.equal(wrongRequest.reason, 'ssh-correlation-request-mismatch');
});

test('an absent pid, helper target or reply is refused rather than defaulted', () => {
  const cases = [
    [remoteDetails({ pid: null }), 'ssh-connected-pid-absent'],
    [remoteDetails({ pid: 0 }), 'ssh-connected-pid-absent'],
    [remoteDetails({ pid: '4242' }), 'ssh-connected-pid-absent'],
    [remoteDetails({ descriptor: { ...remoteDetails().descriptor, target: null } }), 'ssh-helper-target-absent'],
    [remoteDetails({ descriptor: null }), 'ssh-remote-descriptor-absent'],
    [null, 'ssh-remote-session-unknown'],
  ];
  for (const [details, reason] of cases) {
    const correlation = remoteReceiptCorrelation(details, createRecord);
    assert.equal(correlation.ok, false, JSON.stringify(details));
    assert.equal(correlation.reason, reason, JSON.stringify(details));
  }
});

test('a correlated receipt is accepted by the shared validator', () => {
  const receipt = correlatedReceipt();
  assert.equal(receipt.sourceKind, HOST_SUPPLIED_SOURCE_KIND);
  assert.equal(receipt.backendSessionId, daemonSessionId);
  assert.equal(receipt.correlation.helperSessionId, helperSessionId);
  assert.notEqual(receipt.backendSessionId, receipt.correlation.helperSessionId);
  assert.equal(receipt.pid, 4242);
  assert.equal(receipt.executable, '/usr/bin/bash');
  assert.equal(receipt.correlation.source, 'daemon-remote-session-details');

  const checked = validateOwnerHostSpawnReceipt(receipt, {
    hostId: 'host-1',
    transport: 'ssh',
    backendSessionId: daemonSessionId,
    epoch: '41',
    candidateId: candidateProvenance.candidateId,
    sourceManifestSha256: candidateProvenance.sourceManifestSha256,
  });
  assert.equal(checked.ok, true, checked.errors.join('; '));

  // No spawn-time record is claimed, and the pid authorizes nothing.
  assert.equal(receipt.acquisition.spawnedAtProvenance, 'acquisition-observed');
  assert.equal(receipt.acquisition.executableProvenance, 'host-observed-proc-exe');
  assert.match(receipt.acquisition.cleanupAuthority, /^none/);
});

test('a receipt is never built without a confirmed correlation or off Linux', () => {
  const correlation = remoteReceiptCorrelation(remoteDetails(), createRecord);
  const base = {
    correlation,
    hostId: 'host-1',
    epoch: 41,
    remotePlatform: 'linux',
    executable: '/usr/bin/bash',
    candidate: candidateProvenance,
  };
  assert.throws(
    () => remoteOwnerHostReceipt({ ...base, correlation: { ok: false } }),
    (error) => error instanceof ProvisionError && error.reason === 'ssh-receipt-without-correlation',
  );
  // The process identity comes from /proc, so any other platform is refused, not probed.
  for (const platform of ['darwin', 'windows', undefined]) {
    assert.throws(
      () => remoteOwnerHostReceipt({ ...base, remotePlatform: platform }),
      (error) => error.reason === 'ssh-probe-platform-unsupported',
      'platform ' + String(platform) + ' must be refused',
    );
  }
  assert.throws(
    () => remoteOwnerHostReceipt({ ...base, executable: null }),
    (error) => error.reason === 'ssh-executable-unknown',
  );
  assert.equal(SSH_RECEIPT_PROBE_PLATFORM, 'linux');
});

test('the executable probe reads only the exe, and ssh is non-interactive', () => {
  const command = remoteExecutableProbeCommand(4242);
  assert.ok(command.includes('/proc/4242/exe'), command);
  // No start time is read: there is nothing to compare one against, and a start time observed
  // now is not a spawn record.
  assert.ok(!command.includes('stat'), command);
  assert.ok(!command.includes('uptime'), command);

  const args = sshConnectionArgs({ hostname: '127.0.0.1', username: 'qa', port: 44841, identityFile: '/keys/id' });
  assert.ok(args.includes('-T'));
  assert.ok(args.includes('BatchMode=yes'));
  assert.ok(args.includes('StrictHostKeyChecking=yes'));
  assert.ok(args.includes('44841'));
  assert.ok(args.includes('qa@127.0.0.1'));
  assert.ok(args.includes('/keys/id'));
  assert.ok(!args.some((value) => /password/i.test(value)), 'no password path may be offered');
});

/* --------------------------------------------------------------------------
 * The remote spawn the ssh branch sends
 * ------------------------------------------------------------------------ */

const sshWorkspaceId = 'ssh:' + 'c'.repeat(64);
const sshStorePath = '/tmp/herdr-ssh-qa/ssh_hosts.json';
const sshSpawnSession = {
  workspaceId: sshWorkspaceId,
  clientRequestId: 'req-ssh-1',
  remoteCwd: '/home/qa/ulw/stage/repo',
  worktree: null,
  cols: 80,
  rows: 24,
};

test('the remote spawn is the product startup, with no local shell or wrapper', () => {
  const request = remoteSpawnRequest(sshSpawnSession, { id: 'host-ssh' }, 'req-ssh-1', sshStorePath);
  assert.equal(request.type, 'spawn');
  assert.equal(request.workspaceId, sshWorkspaceId);
  assert.equal(request.clientRequestId, 'req-ssh-1');
  // The product's own remote startup, naming the store the daemon is configured with.
  assert.deepEqual(request.startup, { remoteSsh: { hostStorePath: sshStorePath } });
  // A local shell override is refused for an ssh session, and the recording wrapper is a LOCAL
  // artifact the remote host never sees.
  assert.equal(request.shell, null);
  assert.equal(request.worktree, null);
  assert.equal(request.cwd, '/home/qa/ulw/stage/repo');
  assert.equal(request.cols, 80);
  assert.equal(request.rows, 24);
  assert.ok(!('wrapperPath' in request), 'the remote request carries no wrapper');
});

test('a remote spawn requires the derived ssh id and a declared store', () => {
  // The local registry refuses an id containing ':', so a remote spawn must name the ssh id.
  for (const workspaceId of ['plain-workspace', null, 'ssh', 'daemon:1']) {
    assert.throws(
      () => remoteSpawnRequest({ ...sshSpawnSession, workspaceId }, { id: 'host-ssh' }, 'r', sshStorePath),
      (error) => error instanceof ProvisionError && error.reason === 'ssh-workspace-id-required',
      'workspaceId ' + String(workspaceId) + ' must be refused',
    );
  }
  assert.throws(
    () => remoteSpawnRequest(sshSpawnSession, { id: 'host-ssh' }, 'r', null),
    (error) => error.reason === 'ssh-host-store-undeclared',
  );
  assert.throws(
    () => remoteSpawnRequest(sshSpawnSession, { id: 'host-ssh' }, 'r', '   '),
    (error) => error.reason === 'ssh-host-store-undeclared',
  );
  // A leading-space id is trimmed rather than sent as a different workspace than the daemon has.
  assert.equal(
    remoteSpawnRequest({ ...sshSpawnSession, workspaceId: '  ' + sshWorkspaceId }, { id: 'h' }, 'r', sshStorePath).workspaceId,
    sshWorkspaceId,
  );
});

test('the correlation is verified against the real spawn response, not a declaration', () => {
  // What spawnRemotePty records from the daemon's own spawnOk, with no config value involved.
  const fromSpawn = { sessionId: daemonSessionId, clientRequestId: sshSpawnSession.clientRequestId };
  // The daemon's reply reports the request id the spawn was SENT with, so both sides of this
  // comparison come from that ONE value. Hardcoding each side separately is how this fixture
  // drifted, and a drift here is a false mismatch rather than a real refusal.
  const replyForSpawn = remoteDetails({
    descriptor: { ...remoteDetails().descriptor, clientRequestId: sshSpawnSession.clientRequestId },
  });
  const correlated = remoteReceiptCorrelation(replyForSpawn, fromSpawn);
  assert.equal(correlated.ok, true, JSON.stringify(correlated));
  assert.equal(correlated.daemonSessionId, daemonSessionId);

  // A declaration the daemon does not confirm is refused exactly like any other mismatch: a
  // fabricated success is not reachable through this path.
  const fabricated = remoteReceiptCorrelation(remoteDetails(), {
    sessionId: daemonSessionId,
    clientRequestId: 'a-request-the-daemon-never-saw',
  });
  assert.equal(fabricated.ok, false);
  assert.equal(fabricated.reason, 'ssh-correlation-request-mismatch');
  const fabricatedSession = remoteReceiptCorrelation(remoteDetails(), {
    sessionId: 'a-session-the-daemon-never-created',
    clientRequestId: 'req-1',
  });
  assert.equal(fabricatedSession.ok, false);
  assert.equal(fabricatedSession.reason, 'ssh-correlation-session-mismatch');
});

test('the ssh inventory path is a FILE, and the project store is its sibling', () => {
  // daemon_ssh_store_path() returns FERRYX_DATA_DIR/ssh_hosts.json - a file - and the daemon
  // compares that value for exact equality with its own ssh_store_path.
  assert.equal(SSH_HOST_STORE_FILENAME, 'ssh_hosts.json');
  // Built from native pieces, because every derived value is: `join` and `dirname` are
  // platform-native, so a POSIX-spelled input would make `dirname` yield POSIX separators while
  // `sshProjectsStorePath` (which calls `join`) yields native ones - the two then differ on
  // Windows without anything being wrong with the code under test.
  const inventoryDir = join(tmpdir(), 'herdr-ssh-qa');
  const inventory = join(inventoryDir, SSH_HOST_STORE_FILENAME);
  assert.equal(
    remoteSpawnRequest(sshSpawnSession, { id: 'host-ssh' }, 'r', inventory).startup.remoteSsh.hostStorePath,
    inventory,
  );

  // The project store is derived the way the product derives it - `with_file_name` - so it is the
  // inventory file's SIBLING. The child reading would be a path the daemon never looks at.
  // Platform-native: the product derives this with `Path::with_file_name`, so the expected value is
  // built the same way rather than spelled with POSIX separators.
  assert.equal(sshProjectsStorePath(inventory), join(dirname(inventory), SSH_PROJECTS_STORE_FILENAME));
  assert.equal(
    dirname(sshProjectsStorePath(inventory)),
    dirname(inventory),
    'the project store sits beside the inventory file, in the same directory',
  );
  assert.notEqual(
    sshProjectsStorePath(inventory),
    join(dirname(inventory), SSH_HOST_STORE_FILENAME, SSH_PROJECTS_STORE_FILENAME),
    'the project store is never a child of the inventory file',
  );

  // A directory is not this value, and neither is any other filename: the daemon's own path always
  // ends in ssh_hosts.json, so anything else would be refused as an inventory mismatch.
  // The directory, the same directory with a trailing separator, and a differently-named file.
  for (const wrong of [inventoryDir, inventoryDir + sep, join(inventoryDir, 'hosts.json')]) {
    assert.throws(
      () => remoteSpawnRequest(sshSpawnSession, { id: 'host-ssh' }, 'r', wrong),
      (error) => error instanceof ProvisionError && error.reason === 'ssh-host-store-not-inventory-file',
      String(wrong) + ' must be refused as a directory rather than an inventory file',
    );
  }
});

