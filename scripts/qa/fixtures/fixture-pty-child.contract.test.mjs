import { describe, it, expect } from 'vitest';
import { spawn } from 'node:child_process';
import { createConnection } from 'node:net';
import { watch, existsSync } from 'node:fs';
import { mkdtemp, mkdir, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

const FIXTURE_CHILD_SCRIPT = fileURLToPath(new URL('./fixture-pty-child.mjs', import.meta.url));
import { joinFixtureChild } from './native-configured-adapters.mjs';
import { recordExactChild, terminateExactChild, probeOsProcessIdentity } from './paired-isolated-provider.mjs';
import { TransportBarrier } from './transport-provider.mjs';

describe('fixture-pty-child contract', () => {
  it('retained PTY child publishes readiness, enforces scoped correlation, and emits markers directly to stdout', async () => {
    const candidateProbeBinary = process.env.FERRYX_CANDIDATE_BINARY
      || process.env.FERRYX_BINARY
      || null;

    if (!candidateProbeBinary || !existsSync(candidateProbeBinary)) {
      throw new TransportBarrier(
        'CANDIDATE_BINARY_REQUIRED',
        `Explicit candidate probe binary required for OS exact PID probe via FERRYX_CANDIDATE_BINARY or FERRYX_BINARY: ${candidateProbeBinary ?? '(unset)'}`
      );
    }

    const tempProfile = await mkdtemp(join(tmpdir(), 'fixture-child-test-'));
    const receiptsDir = join(tempProfile, 'receipts');
    await mkdir(receiptsDir, { recursive: true });

    const ownedClients = [];
    let childProc;
    let recordedChild = null;
    let readyWatcher = null;
    let readyTimer = null;

    try {
      const runId = 'test-run-123';
      const operationId = 'test-op-456';

      // Arm readiness event watch on receipts directory BEFORE spawn (zero polling)
      let readyResolver;
      let readyRejecter;
      const readyPromise = new Promise((resolve, reject) => {
        readyResolver = resolve;
        readyRejecter = reject;
      });

      readyTimer = setTimeout(() => {
        readyRejecter(new Error('Timed out waiting for fixture-child-ready.json'));
      }, 10000);

      readyWatcher = watch(receiptsDir, async (eventType, filename) => {
        if (filename === 'fixture-child-ready.json' || !filename) {
          try {
            const raw = JSON.parse(await readFile(join(receiptsDir, 'fixture-child-ready.json'), 'utf8'));
            if (raw && raw.status === 'ready' && raw.endpoint) {
              if (readyTimer) {
                clearTimeout(readyTimer);
                readyTimer = null;
              }
              readyWatcher?.close();
              readyWatcher = null;
              readyResolver(raw);
            }
          } catch (_) {}
        }
      });

      // Spawn fixture child simulating Ferryx daemon PTY spawn
      childProc = spawn(process.execPath, [FIXTURE_CHILD_SCRIPT], {
        env: {
          ...process.env,
          FERRYX_QA_PROFILE: tempProfile,
          FERRYX_QA_RUN_ID: runId,
          FERRYX_QA_OPERATION_ID: operationId,
        },
        stdio: ['ignore', 'pipe', 'pipe'],
      });

      // Immediately record exact OS identity (PID, executablePath, processStart) via production probe
      recordedChild = await recordExactChild({
        child: childProc,
        expectedBinary: process.execPath,
        role: 'fixture-child-test',
        probe: (pid) => probeOsProcessIdentity(pid, candidateProbeBinary),
      });
      expect(Number.isSafeInteger(recordedChild.pid) && recordedChild.pid > 0).toBe(true);
      expect(recordedChild.executablePath).toBeTruthy();
      expect(recordedChild.processStart).toBeTruthy();

      // Event-driven capture and line-waiter subscriptions for child stdout (zero polling)
      const stdoutLines = [];
      let pendingWaiters = [];
      let stdoutBuf = '';

      childProc.stdout.on('data', (d) => {
        stdoutBuf += d.toString('utf8');
        let nl;
        while ((nl = stdoutBuf.indexOf('\n')) !== -1) {
          const line = stdoutBuf.slice(0, nl).trim();
          stdoutBuf = stdoutBuf.slice(nl + 1);
          if (line) {
            stdoutLines.push(line);
            for (const waiter of [...pendingWaiters]) {
              if (waiter.marker === line) {
                waiter.resolve();
              }
            }
          }
        }
      });

      function subscribeStdoutMarker(marker, timeoutMs = 5000) {
        if (stdoutLines.includes(marker)) return Promise.resolve();
        return new Promise((resolve, reject) => {
          const timer = setTimeout(() => {
            pendingWaiters = pendingWaiters.filter((w) => w !== waiter);
            reject(new Error(`Timed out waiting for stdout marker: ${marker}`));
          }, timeoutMs);
          const waiter = {
            marker,
            resolve: () => {
              clearTimeout(timer);
              pendingWaiters = pendingWaiters.filter((w) => w !== waiter);
              resolve();
            },
          };
          pendingWaiters.push(waiter);
        });
      }

      // Await readiness receipt from pre-armed watcher
      const readyReceipt = await readyPromise;

      expect(readyReceipt).not.toBeNull();
      expect(readyReceipt.status).toBe('ready');
      expect(readyReceipt.pid).toBe(childProc.pid);
      expect(readyReceipt.runId).toBe(runId);
      expect(readyReceipt.operationId).toBe(operationId);
      expect(typeof readyReceipt.readyTimestamp).toBe('number');

      // Connect to event-driven control IPC
      const isWindows = process.platform === 'win32';
      const client = isWindows
        ? createConnection({ host: '127.0.0.1', port: readyReceipt.endpoint.port })
        : createConnection({ path: readyReceipt.endpoint.path });
      ownedClients.push(client);

      await new Promise((res, rej) => {
        client.once('connect', res);
        client.once('error', rej);
      });

      function sendCommand(cmd) {
        return new Promise((resolve, reject) => {
          let buf = '';
          const onData = (chunk) => {
            buf += chunk.toString('utf8');
            const nl = buf.indexOf('\n');
            if (nl !== -1) {
              client.removeListener('data', onData);
              try { resolve(JSON.parse(buf.slice(0, nl))); } catch (e) { reject(e); }
            }
          };
          client.on('data', onData);
          client.write(JSON.stringify(cmd) + '\n');
        });
      }

      // Negative Test 1: Mismatched runId -> refused with SCOPE_MISMATCH
      const scopeMismatch = await sendCommand({
        type: 'emit-marker',
        marker: 'unauthorized-marker',
        runId: 'wrong-run-id',
        operationId,
      });
      expect(scopeMismatch.type).toBe('error');
      expect(scopeMismatch.code).toBe('SCOPE_MISMATCH');
      expect(stdoutLines).not.toContain('unauthorized-marker');

      // Negative Test 2: Mismatched operationId -> refused with SCOPE_MISMATCH
      const opMismatch = await sendCommand({
        type: 'emit-marker',
        marker: 'unauthorized-marker-2',
        runId,
        operationId: 'wrong-op-id',
      });
      expect(opMismatch.type).toBe('error');
      expect(opMismatch.code).toBe('SCOPE_MISMATCH');
      expect(stdoutLines).not.toContain('unauthorized-marker-2');

      // Positive Test 1: Subscribe stdout marker signal BEFORE sendCommand to avoid socket reply vs stdout race
      const stdoutPromiseAscii = subscribeStdoutMarker('MARKER_ASCII_001');
      const okAscii = await sendCommand({
        type: 'emit-marker',
        marker: 'MARKER_ASCII_001',
        runId,
        operationId,
      });
      expect(okAscii.type).toBe('marker-emitted');
      expect(okAscii.marker).toBe('MARKER_ASCII_001');
      expect(okAscii.pid).toBe(childProc.pid);
      await stdoutPromiseAscii;
      expect(stdoutLines).toContain('MARKER_ASCII_001');

      // Positive Test 2: Multi-byte UTF-8 marker split across byte chunks
      const multiByteMarker = 'MARKER_한글_🔥_테스트_002';
      const cmdPayload = Buffer.from(JSON.stringify({
        type: 'emit-marker',
        marker: multiByteMarker,
        runId,
        operationId,
      }) + '\n', 'utf8');

      const split = Math.floor(cmdPayload.length / 2);
      const replyPromise = new Promise((resolve) => {
        let buf = '';
        const onData = (chunk) => {
          buf += chunk.toString('utf8');
          const nl = buf.indexOf('\n');
          if (nl !== -1) {
            client.removeListener('data', onData);
            resolve(JSON.parse(buf.slice(0, nl)));
          }
        };
        client.on('data', onData);
      });

      // Subscribe stdout marker signal BEFORE sending chunked payload
      const stdoutPromiseUtf8 = subscribeStdoutMarker(multiByteMarker);

      // Write chunk 1 and chunk 2 yielding event loop between chunks (no fixed sleeps)
      client.write(cmdPayload.subarray(0, split));
      await new Promise((r) => setImmediate(r));
      client.write(cmdPayload.subarray(split));

      const okUtf8 = await replyPromise;
      expect(okUtf8.type).toBe('marker-emitted');
      expect(okUtf8.marker).toBe(multiByteMarker);
      await stdoutPromiseUtf8;
      expect(stdoutLines).toContain(multiByteMarker);

    } finally {
      if (readyTimer) {
        clearTimeout(readyTimer);
        readyTimer = null;
      }
      if (readyWatcher) {
        readyWatcher.close();
        readyWatcher = null;
      }
      for (const client of ownedClients) {
        client.destroy();
      }
      if (childProc && recordedChild) {
        await terminateExactChild({
          child: childProc,
          recorded: recordedChild,
          probe: (pid) => probeOsProcessIdentity(pid, candidateProbeBinary),
          timeoutMs: 5000,
        });
      }
      await rm(tempProfile, { recursive: true, force: true }).catch(() => {});
    }
  });
  it('Windows parent chain: verifies descendant fixture child with distinct root and rejects mismatches via joinFixtureChild', async () => {
    const rootTarget = {
      pid: 1000,
      processStart: '20261014-100000',
      executable: 'C:\\Windows\\System32\\cmd.exe',
    };
    const expectedExecutable = 'C:\\Users\\sook\\.bun\\bin\\bun.exe';
    const recordedPids = [];
    const recordOwnedPid = async (p) => recordedPids.push(p);

    // Positive: Valid descendant matching parent chain, matching scope, and exact expected executable
    const validReceipt = {
      pid: 1001,
      runId: 'run-1',
      operationId: 'op-1',
      endpoint: { port: 5000 },
    };
    const validLive = {
      exited: false,
      pid: 1001,
      ppid: 1000,
      processStart: '20261014-100001',
      executable: expectedExecutable,
    };
    const joined = await joinFixtureChild({
      target: { ...rootTarget },
      fixtureChild: validReceipt,
      platform: 'win32',
      probe: async () => validLive,
      recordOwnedPid,
      expectedExecutable,
      expectedRunId: 'run-1',
      expectedOperationId: 'op-1',
    });
    expect(joined.rootPid).toBe(1000);
    expect(joined.descendantPid).toBe(1001);
    expect(joined.fixtureChild.ppid).toBe(1000);
    expect(joined.fixtureChild.executable).toBe(expectedExecutable);
    expect(recordedPids.some((p) => p.pid === 1001 && p.role === 'fixture-child')).toBe(true);

    // Negative 1: Mismatched PPID (not a child of PTY root)
    const badPpidLive = { ...validLive, ppid: 9999 };
    await expect(joinFixtureChild({
      target: { ...rootTarget },
      fixtureChild: validReceipt,
      platform: 'win32',
      probe: async () => badPpidLive,
      expectedExecutable,
      expectedRunId: 'run-1',
      expectedOperationId: 'op-1',
    })).rejects.toThrow(/is not descendant of PTY root/);

    // Negative 2: Child start before root start (clock/time-travel violation)
    const preRootLive = { ...validLive, processStart: '20261014-095959' };
    await expect(joinFixtureChild({
      target: { ...rootTarget },
      fixtureChild: validReceipt,
      platform: 'win32',
      probe: async () => preRootLive,
      expectedExecutable,
      expectedRunId: 'run-1',
      expectedOperationId: 'op-1',
    })).rejects.toThrow(/started before PTY root process/);

    // Negative 3: Executable mismatch
    const badExeLive = { ...validLive, executable: 'C:\\bad\\cmd.exe' };
    await expect(joinFixtureChild({
      target: { ...rootTarget },
      fixtureChild: validReceipt,
      platform: 'win32',
      probe: async () => badExeLive,
      expectedExecutable,
      expectedRunId: 'run-1',
      expectedOperationId: 'op-1',
    })).rejects.toThrow(/does not match expected/);

    // Negative 4: Scope mismatch (runId)
    await expect(joinFixtureChild({
      target: { ...rootTarget },
      fixtureChild: { ...validReceipt, runId: 'wrong-run' },
      platform: 'win32',
      probe: async () => validLive,
      expectedExecutable,
      expectedRunId: 'run-1',
      expectedOperationId: 'op-1',
    })).rejects.toThrow(/runId wrong-run does not match expected run-1/);
  });

  it('Unix root exec: verifies same PID transition and rejects PID / executable mismatches via joinFixtureChild', async () => {
    const rootTarget = {
      pid: 5000,
      processStart: '20261014-120000',
      executable: '/bin/sh',
    };
    const expectedExecutable = '/opt/homebrew/bin/bun';

    // Positive: Child PID matches root PID (execve replaced image in place)
    const validReceipt = {
      pid: 5000,
      runId: 'run-1',
      operationId: 'op-1',
      endpoint: { path: '/tmp/test.sock' },
    };
    const validLive = {
      exited: false,
      pid: 5000,
      processStart: '20261014-120000',
      executable: expectedExecutable,
    };
    const joined = await joinFixtureChild({
      target: { ...rootTarget },
      fixtureChild: validReceipt,
      platform: 'darwin',
      probe: async () => validLive,
      expectedExecutable,
      expectedRunId: 'run-1',
      expectedOperationId: 'op-1',
    });
    expect(joined.rootPid).toBe(5000);
    expect(joined.descendantPid).toBe(5000);
    expect(joined.fixtureChild.executable).toBe(expectedExecutable);

    // Negative 1: PID mismatch on Unix
    const mismatchedReceipt = { ...validReceipt, pid: 5001 };
    const mismatchedLive = { ...validLive, pid: 5001 };
    await expect(joinFixtureChild({
      target: { ...rootTarget },
      fixtureChild: mismatchedReceipt,
      platform: 'darwin',
      probe: async () => mismatchedLive,
      expectedExecutable,
      expectedRunId: 'run-1',
      expectedOperationId: 'op-1',
    })).rejects.toThrow(/does not match PTY root/);

    // Negative 2: Executable mismatch on Unix (failed to transition to bun)
    const badExeLive = { ...validLive, executable: '/bin/sh' };
    await expect(joinFixtureChild({
      target: { ...rootTarget },
      fixtureChild: validReceipt,
      platform: 'darwin',
      probe: async () => badExeLive,
      expectedExecutable,
      expectedRunId: 'run-1',
      expectedOperationId: 'op-1',
    })).rejects.toThrow(/does not match expected/);
  });
});
