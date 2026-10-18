/**
 * Real paired isolated daemon provisioning adapter.
 * Spawns separate host and client daemons in isolated profiles,
 * pairs them using authentic PairedHostPair/Operation APIs,
 * returns measured identities and recorded PIDs,
 * and supports cold client-daemon restart while preserving host PTY.
 */

import { spawn, spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { existsSync } from 'node:fs';
import { readFile, realpath } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import {
  setupIsolatedProfile,
  connectDaemonControl,
  daemonControlTransport,
  seedIsolatedRemoteConfig,
  writeJson,
  executablePathMatches,
  REMOTE_GATEWAY_PORT,
  DAEMON_READY_LINE,
  repoRoot,
} from '../herdr-reference-fixtures.mjs';
import { probeDarwin } from './native-remote-agent.mjs';
import { TransportBarrier, pairedOwnerControls as basePairedOwnerControls, TRANSPORT_PROVIDER_SCHEMA } from './transport-provider.mjs';

export function pairedOwnerControls(control, eventControl = control) {
  const owner = basePairedOwnerControls(control);
  const events = basePairedOwnerControls(eventControl);
  return {
    ...owner,
    waitState: (...args) => events.waitState(...args),
    async mutate(session, operation, data = Buffer.alloc(0), intentId) {
      const { backendSessionId, daemonEpoch, incarnation, generation } = session.identity;
      if (!intentId || !Number.isSafeInteger(generation)) {
        throw new TransportBarrier('PAIRED_MUTATION_IDENTITY_REQUIRED', 'original intentId and actual controller generation');
      }
      const response = await control.call({ type: 'qaPairedMutation', backendSessionId,
        daemonEpoch, incarnation, generation, operation, intentId,
        data: Buffer.from(data).toString('base64') });
      if (response.type !== 'qaPairedMutationOk' || response.intentId !== intentId) {
        throw new TransportBarrier('PAIRED_MUTATION_UNAVAILABLE', JSON.stringify(response));
      }
      return response;
    },
  };
}

/**
 * Client adapter for the authoritative host machine delivery observer.
 * Operates over hostControl UDS connection with full concrete target identity,
 * strict deviceId validation, concrete generation, armed+sealed finite fault interval,
 * and expected multiplicity bounds.
 */
export function pairedHostDeliveryObserver(hostControl) {
  function formatTarget(target) {
    if (!target?.machineId || !target?.daemonEpoch || !target?.sessionId) {
      throw new TransportBarrier('TARGET_IDENTITY_INCOMPLETE', 'target requires machineId, daemonEpoch, sessionId');
    }
    return {
      machineId: String(target.machineId),
      daemonEpoch: String(target.daemonEpoch),
      sessionId: String(target.sessionId),
    };
  }

  return {
    async armFence({ target, deviceId, generation, expectedWriteMultiplicity, expectedSignalMultiplicity }) {
      const concreteTarget = formatTarget(target);
      if (typeof deviceId !== 'string' || !deviceId.trim().length) {
        throw new TransportBarrier('DEVICE_IDENTITY_REQUIRED', 'armFence requires explicit non-empty deviceId');
      }
      if (generation === undefined || generation === null || String(generation).trim().length === 0) {
        throw new TransportBarrier('GENERATION_REQUIRED', 'armFence requires explicit valid concrete generation');
      }
      const res = await hostControl.call({
        type: 'qaPairedHostDeliveryArmFence',
        target: concreteTarget,
        deviceId: String(deviceId).trim(),
        generation: String(generation).trim(),
        expectedWriteMultiplicity: expectedWriteMultiplicity ?? null,
        expectedSignalMultiplicity: expectedSignalMultiplicity ?? null,
      });
      if (res.type !== 'qaPairedHostDeliveryArmFenceOk' || !res.fenceId) {
        throw new TransportBarrier('TRAFFIC_AUDITOR_PRODUCER_REQUIRED', `host arm fence failed: ${JSON.stringify(res)}`);
      }
      return res.fenceId;
    },
    async sealFence({ fenceId, target }) {
      if (!fenceId) {
        throw new TransportBarrier('FENCE_ID_REQUIRED', 'sealFence requires explicit fenceId');
      }
      const concreteTarget = formatTarget(target);
      const res = await hostControl.call({
        type: 'qaPairedHostDeliverySealFence',
        fenceId,
        target: concreteTarget,
      });
      if (res.type !== 'qaPairedHostDeliverySealFenceOk' || !res.fence) {
        throw new TransportBarrier('TRAFFIC_AUDITOR_PRODUCER_REQUIRED', `host seal fence failed: ${JSON.stringify(res)}`);
      }
      return res.fence;
    },
    async audit({ target, fenceId }) {
      if (!fenceId) {
        throw new TransportBarrier('FENCE_ID_REQUIRED', 'audit requires explicit fenceId');
      }
      const concreteTarget = formatTarget(target);
      const res = await hostControl.call({
        type: 'qaPairedHostDeliveryAudit',
        target: concreteTarget,
        fenceId,
      });
      if (res.type !== 'qaPairedHostDeliveryAuditOk' || !res.audit) {
        throw new TransportBarrier('TRAFFIC_AUDITOR_PRODUCER_REQUIRED', `host delivery audit failed: ${JSON.stringify(res)}`);
      }
      return res.audit;
    },
  };
}

/**
 * Probe live OS process identity (PID, executable path, processStart token).
 * Throws typed TransportBarrier if probe is unavailable or identity cannot be determined.
 */
export async function probeOsProcessIdentity(pid, binary, signal) {
  if (!Number.isSafeInteger(pid) || pid <= 0) {
    throw new TransportBarrier('OWNED_PROCESS_IDENTITY_UNAVAILABLE', `Invalid exact PID ${pid}`);
  }
  const resolvedBinary = binary ? resolve(binary) : null;
  if (!resolvedBinary) {
    throw new TransportBarrier('OWNED_PROCESS_IDENTITY_UNAVAILABLE', `Candidate binary required for OS probe of PID ${pid}`);
  }
  if (process.platform === 'darwin') {
    try {
      const val = await probeDarwin(pid, resolvedBinary, signal);
      if (val.exited || !val.executable || !val.processStart) {
        throw new TransportBarrier('OWNED_PROCESS_IDENTITY_UNAVAILABLE', `Darwin libproc identity incomplete for PID ${pid}`);
      }
      return { pid, exited: false, executable: val.executable, processStart: val.processStart };
    } catch (err) {
      if (err instanceof TransportBarrier) throw err;
      throw new TransportBarrier('OWNED_PROCESS_IDENTITY_UNAVAILABLE', `Darwin probe failed: ${err.message}`);
    }
  }
  if (process.platform === 'linux') {
    try {
      const stat = await readFile(`/proc/${pid}/stat`, 'utf8');
      const fields = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
      const processStart = fields[19];
      const executable = await realpath(`/proc/${pid}/exe`);
      if (!processStart || !executable) {
        throw new TransportBarrier('OWNED_PROCESS_IDENTITY_UNAVAILABLE', `Linux identity incomplete for PID ${pid}`);
      }
      return { pid, exited: false, executable, processStart };
    } catch (err) {
      if (err.code === 'ENOENT') return { pid, exited: true, executable: null, processStart: null };
      throw new TransportBarrier('OWNED_PROCESS_IDENTITY_UNAVAILABLE', `Linux probe failed: ${err.message}`);
    }
  }
  if (process.platform === 'win32') {
    try {
      const script = `$p=Get-Process -Id ${pid} -ErrorAction SilentlyContinue; if($null -eq $p){'{"exited":true}'}else{$w=Get-CimInstance Win32_Process -Filter "ProcessId = ${pid}"; @{exited=$false;executable=$p.Path;processStart=$p.StartTime.ToUniversalTime().Ticks.ToString()}|ConvertTo-Json -Compress}`;
      const res = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8' });
      if (res.status !== 0) throw new Error(res.stderr || 'powershell probe failed');
      const val = JSON.parse(res.stdout.trim());
      if (val.exited) return { pid, exited: true, executable: null, processStart: null };
      if (!val.executable || !val.processStart) {
        throw new TransportBarrier('OWNED_PROCESS_IDENTITY_UNAVAILABLE', `Windows identity incomplete for PID ${pid}`);
      }
      return { pid, exited: false, executable: val.executable, processStart: val.processStart };
    } catch (err) {
      throw new TransportBarrier('OWNED_PROCESS_IDENTITY_UNAVAILABLE', `Windows probe failed: ${err.message}`);
    }
  }
  throw new TransportBarrier('OWNED_PROCESS_IDENTITY_UNAVAILABLE', `Platform ${process.platform} OS identity probe unavailable`);
}

function waitForReady(child, timeoutMs = 30000) {
  let timer;
  let onData;
  let onErr;
  let onExit;
  const dispose = () => {
    if (timer) clearTimeout(timer);
    if (onData) child.stdout?.off('data', onData);
    if (onErr) child.stderr?.off('data', onErr);
    if (onExit) child.off('exit', onExit);
  };

  const promise = new Promise((resolveReady, rejectReady) => {
    let lineBuffer = '';
    let stderr = '';
    timer = setTimeout(() => {
      dispose();
      rejectReady(new TransportBarrier('DAEMON_BOOT_TIMEOUT', `stderr: ${stderr}`));
    }, timeoutMs);

    onData = (data) => {
      lineBuffer += data.toString();
      let idx;
      while ((idx = lineBuffer.indexOf('\n')) !== -1) {
        const line = lineBuffer.slice(0, idx).trim();
        lineBuffer = lineBuffer.slice(idx + 1);
        if (line === DAEMON_READY_LINE) {
          dispose();
          resolveReady(true);
          return;
        }
      }
    };
    onErr = (data) => {
      stderr = (stderr + data.toString()).slice(-64 * 1024);
    };

    onExit = (code, signal) => {
      dispose();
      rejectReady(new TransportBarrier('DAEMON_PREMATURE_EXIT', `code: ${code}, signal: ${signal}, stderr: ${stderr}`));
    };

    child.stdout?.on('data', onData);
    child.stderr?.on('data', onErr);
    child.once('exit', onExit);
  });

  return { promise, dispose };
}

/**
 * Record exact OS process identity of a spawned child process.
 * Verifies live process identity and confirms executable matches expectedBinary.
 */
export async function recordExactChild({ child, expectedBinary, role, probe, ledger }) {
  if (!child || !Number.isSafeInteger(child.pid) || child.pid <= 0) {
    throw new TransportBarrier('OWNED_PROCESS_IDENTITY_UNAVAILABLE', 'Valid child process with positive PID required');
  }
  let live;
  try {
    live = await probe(child.pid);
  } catch (error) {
    throw new TransportBarrier('OWNED_PROCESS_IDENTITY_UNAVAILABLE', `OS process identity probe failed for PID ${child.pid}: ${error?.message ?? error}`);
  }
  if (!live || live.exited || !live.processStart || !live.executable) {
    throw new TransportBarrier(
      'OWNED_PROCESS_IDENTITY_UNAVAILABLE',
      `OS process identity probe unavailable for PID ${child.pid}`,
    );
  }
  if (expectedBinary && !executablePathMatches(live.executable, expectedBinary, process.platform)) {
    throw new TransportBarrier(
      'OWNED_PROCESS_IDENTITY_MISMATCH',
      `PID ${child.pid} executable (${live.executable}) does not match expected candidate binary (${expectedBinary})`,
    );
  }
  const extra = {
    role,
    executablePath: live.executable,
    processStart: live.processStart,
    argv: child.spawnargs ?? ['--daemon'],
    spawnedAt: new Date().toISOString(),
  };
  if (ledger && typeof ledger.record === 'function') {
    ledger.record(child, extra);
  }
  return {
    pid: child.pid,
    executablePath: live.executable,
    processStart: live.processStart,
    role,
  };
}

/**
 * Terminate an exact owned child process.
 * Verifies live process ownership matches recorded identity before dispatching signal.
 * Subscribes bounded exit before signaling; rejects with typed barrier on timeout.
 * Never performs unchecked secondary kill or SIGKILL fallback.
 */
export async function terminateExactChild({
  child,
  recorded,
  probe,
  timeoutMs = 10000,
  signal,
  signalProcess,
}) {
  if (!child || !recorded) return { exited: true };
  if (child.pid !== recorded.pid) {
    throw new TransportBarrier(
      'OWNED_PROCESS_IDENTITY_MISMATCH',
      `Child PID ${child.pid} does not match recorded PID ${recorded.pid}`,
      { childPid: child.pid, recordedPid: recorded.pid },
    );
  }
  if (child.exitCode !== null || child.signalCode !== null) {
    return { pid: recorded.pid, exited: true, code: child.exitCode, signal: child.signalCode };
  }

  let live;
  try {
    live = await probe(child.pid);
  } catch (probeErr) {
    throw new TransportBarrier(
      'OWNED_PROCESS_IDENTITY_UNAVAILABLE',
      `OS probe unavailable for PID ${child.pid}: ${probeErr.message}`,
    );
  }
  if (!live || typeof live !== 'object') {
    throw new TransportBarrier(
      'OWNED_PROCESS_IDENTITY_UNAVAILABLE',
      `OS probe returned invalid result for PID ${child.pid}`,
    );
  }
  if (live.exited === true) {
    return { pid: recorded.pid, exited: true };
  }
  if (
    live.processStart !== recorded.processStart ||
    !executablePathMatches(live.executable, recorded.executablePath, process.platform)
  ) {
    throw new TransportBarrier(
      'OWNED_PROCESS_IDENTITY_MISMATCH',
      `PID ${child.pid} live identity (start=${live.processStart}, exe=${live.executable}) disagrees with recorded (start=${recorded.processStart}, exe=${recorded.executablePath})`,
    );
  }

  // Subscribe bounded exit listener BEFORE signaling
  let timer;
  let cleanExitListener;
  const exitPromise = new Promise((resolveExit, rejectExit) => {
    const onExit = (code, sig) => {
      cleanExitListener();
      resolveExit({ code, signal: sig });
    };
    const onError = (err) => {
      cleanExitListener();
      rejectExit(err);
    };
    cleanExitListener = () => {
      if (timer) clearTimeout(timer);
      if (typeof child.removeListener === 'function') {
        child.removeListener('exit', onExit);
        child.removeListener('error', onError);
      } else if (typeof child.off === 'function') {
        child.off('exit', onExit);
        child.off('error', onError);
      }
    };
    child.once('exit', onExit);
    child.once('error', onError);

    timer = setTimeout(() => {
      cleanExitListener();
      rejectExit(new TransportBarrier(
        'DAEMON_EXIT_TIMEOUT',
        `PID ${child.pid} failed to exit within ${timeoutMs}ms after termination signal; owned resource: ${JSON.stringify(recorded)}`,
      ));
    }, timeoutMs);
  });

  // Guard: if child already exited during probe / setup before listener attached
  if (child.exitCode !== null || child.signalCode !== null) {
    cleanExitListener();
    return { pid: recorded.pid, exited: true, code: child.exitCode, signal: child.signalCode };
  }

  // Send single termination signal (NO unchecked SIGKILL / taskkill repeat)
  try {
    if (typeof signalProcess === 'function') {
      const res = signalProcess(child.pid, 'SIGTERM');
      if (res === false) {
        cleanExitListener();
        throw new TransportBarrier('TERMINATION_SIGNAL_FAILED', `signalProcess returned false for PID ${child.pid}`);
      }
    } else if (typeof child.kill === 'function') {
      const res = child.kill('SIGTERM');
      if (res === false) {
        cleanExitListener();
        throw new TransportBarrier('TERMINATION_SIGNAL_FAILED', `child.kill returned false for PID ${child.pid}`);
      }
    } else if (process.platform === 'win32') {
      const res = spawnSync('taskkill', ['/PID', String(child.pid), '/F']);
      if (res.status !== 0) {
        cleanExitListener();
        throw new TransportBarrier('TERMINATION_SIGNAL_FAILED', `taskkill exited with status ${res.status}: ${res.stderr || ''}`);
      }
    } else {
      const res = process.kill(child.pid, 'SIGTERM');
      if (res === false) {
        cleanExitListener();
        throw new TransportBarrier('TERMINATION_SIGNAL_FAILED', `process.kill returned false for PID ${child.pid}`);
      }
    }
  } catch (sigErr) {
    cleanExitListener();
    if (sigErr instanceof TransportBarrier) throw sigErr;
    throw new TransportBarrier('TERMINATION_SIGNAL_FAILED', `Failed to signal PID ${child.pid}: ${sigErr.message}`);
  }

  // Bounded wait for exit; timeout rejects with DAEMON_EXIT_TIMEOUT without second kill
  const exitResult = await exitPromise;
  return { pid: recorded.pid, exited: true, ...exitResult };
}

/**
 * Authoritative wire request builders conforming to Rust remote/machine_protocol DTOs.
 */
export function buildCreateSessionRequest({
  requestId = randomUUID(),
  workspaceId,
  worktree = null,
  cols = 80,
  rows = 24,
  inheritFromSessionId = null,
  cwdRelative = null,
  startup = { kind: 'shell' },
} = {}) {
  return {
    requestId,
    workspaceId,
    worktree,
    cols,
    rows,
    inheritFromSessionId,
    cwdRelative,
    startup,
  };
}

export function buildDescriptor({
  hostId,
  generation,
  target,
  afterSequence = null,
} = {}) {
  return {
    hostId,
    generation,
    target,
    afterSequence,
  };
}

export function buildPairedRecoveryOutcomeRequest(backendSessionId) {
  return {
    type: 'pairedRecoveryOutcome',
    backend_session_id: backendSessionId,
  };
}

/**
 * Launch an isolated paired harness with separate host and client daemons.
 */
export async function launchPairedIsolatedHarness(options = {}) {
  const binary = options.binary ? resolve(options.binary) : null;
  if (!binary || !existsSync(binary)) {
    throw new TransportBarrier('CANDIDATE_BINARY_REQUIRED', String(options.binary || ''));
  }
  const timeoutMs = typeof options.timeoutMs === 'number' ? options.timeoutMs : 30000;
  const ledger = options.ledger;
  const probe = options.probe ?? ((pid) => probeOsProcessIdentity(pid, binary));

  const hostProfile = options.hostProfile ?? setupIsolatedProfile('paired-isolated-host');
  const clientProfile = options.clientProfile ?? setupIsolatedProfile('paired-isolated-client');

  // Seed host config: localNetwork gateway enabled on REMOTE_GATEWAY_PORT
  seedIsolatedRemoteConfig(hostProfile);

  // Seed client config: gateway off (prevents port collision on REMOTE_GATEWAY_PORT)
  const clientConfigPath = join(clientProfile.paths.data, 'remote', 'remote-config.json');
  writeJson(clientConfigPath, { mode: 'off' });

  let hostChild = null;
  let hostRecorded = null;
  let hostReadyDisposer = null;
  let clientChild = null;
  let clientRecorded = null;
  let clientReadyDisposer = null;

  let hostControl = null;
  let clientControl = null;
  let eventControl = null;

  try {
    // Spawn host daemon inside guarded lifecycle
    hostChild = spawn(binary, ['--daemon'], {
      cwd: repoRoot,
      env: hostProfile.env,
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true,
    });
    const hostReady = waitForReady(hostChild, timeoutMs);
    hostReadyDisposer = hostReady.dispose;
    hostRecorded = await recordExactChild({
      child: hostChild,
      expectedBinary: binary,
      role: 'paired-host-daemon',
      probe,
      ledger,
    });

    // Spawn client daemon inside guarded lifecycle
    clientChild = spawn(binary, ['--daemon'], {
      cwd: repoRoot,
      env: clientProfile.env,
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true,
    });
    const clientReady = waitForReady(clientChild, timeoutMs);
    clientReadyDisposer = clientReady.dispose;
    clientRecorded = await recordExactChild({
      child: clientChild,
      expectedBinary: binary,
      role: 'paired-client-daemon',
      probe,
      ledger,
    });

    await Promise.all([
      hostReady.promise,
      clientReady.promise,
    ]);

    hostControl = await connectDaemonControl(daemonControlTransport(hostProfile), { timeoutMs });
    clientControl = await connectDaemonControl(daemonControlTransport(clientProfile), { timeoutMs });
    eventControl = await connectDaemonControl(daemonControlTransport(clientProfile), { timeoutMs });

    // Step 1: Create machine pairing code on host daemon
    const pinRes = await hostControl.call({ type: 'remoteCreateMachinePairingCode' });
    if (pinRes.type !== 'remotePairingCodeOk' || !pinRes.code) {
      throw new TransportBarrier('REMOTE_PAIRING_CODE_FAILED', JSON.stringify(pinRes));
    }
    const pin = pinRes.code;

    // Step 2: Pair client daemon with host daemon
    const hostOrigin = `http://127.0.0.1:${REMOTE_GATEWAY_PORT}`;
    const pairRes = await clientControl.call({
      type: 'pairedHostPair',
      request: { relayOrigin: hostOrigin, pin, displayLabel: 'isolated-host' },
    });
    if (pairRes.type !== 'pairedHostPairOk' || !pairRes.host) {
      throw new TransportBarrier('PAIRED_HOST_PAIR_FAILED', JSON.stringify(pairRes));
    }
    const host = pairRes.host;

    // Step 3: Register existing task-owned git repository on host via client operation
    const repoDir = options.repoPath ? resolve(options.repoPath) : repoRoot;

    const regRes = await clientControl.call({
      type: 'pairedHostOperation',
      request: {
        hostId: host.hostId,
        generation: host.generation,
        operation: { kind: 'registerProject', request: { requestId: randomUUID(), repoPath: repoDir } },
      },
    });
    if (regRes.type !== 'pairedHostOperationOk' || regRes.response?.result?.kind !== 'registerProject' || !regRes.response?.result?.data?.remoteWorkspaceId) {
      throw new TransportBarrier('PROJECT_REGISTRATION_FAILED', JSON.stringify(regRes));
    }
    const project = regRes.response.result.data;
    const workspaceId = project.remoteWorkspaceId;

    // Step 4: Spawn session on host via client operation
    const sessionRes = await clientControl.call({
      type: 'pairedHostOperation',
      request: {
        hostId: host.hostId,
        generation: host.generation,
        operation: {
          kind: 'createSession',
          request: buildCreateSessionRequest({
            workspaceId,
            worktree: null,
            cols: 80,
            rows: 24,
            inheritFromSessionId: null,
            cwdRelative: null,
            startup: { kind: 'shell' },
          }),
        },
      },
    });
    if (sessionRes.type !== 'pairedHostOperationOk' || sessionRes.response?.result?.kind !== 'createSession' || !sessionRes.response?.result?.data?.target?.sessionId) {
      throw new TransportBarrier('SESSION_SPAWN_FAILED', JSON.stringify(sessionRes));
    }
    const sessionData = sessionRes.response.result.data;
    const target = sessionData.target;

    // Step 5: Attach paired proxy on client daemon
    const descriptor = buildDescriptor({ hostId: host.hostId, generation: host.generation, target, afterSequence: null });
    const reattachRes = await clientControl.call({
      type: 'pairedTerminalReattach',
      descriptor,
    });
    const backendSessionId = reattachRes.session_id;
    if (reattachRes.type !== 'pairedTerminalReattachOk' || typeof backendSessionId !== 'string' || !backendSessionId.length) {
      throw new TransportBarrier('PAIRED_REATTACH_FAILED', JSON.stringify(reattachRes));
    }

    // Step 6: Verify and extract measured recovery outcome
    const outcomeRes = await clientControl.call(buildPairedRecoveryOutcomeRequest(backendSessionId));
    if (outcomeRes.type !== 'pairedRecoveryOutcomeResult' || !outcomeRes.outcome) {
      throw new TransportBarrier('PAIRED_RECOVERY_OUTCOME_FAILED', JSON.stringify(outcomeRes));
    }
    const outcome = outcomeRes.outcome;
    if (!outcome.daemonEpoch || !outcome.incarnation) {
      throw new TransportBarrier(
        'PAIRED_MEASURED_IDENTITY_REQUIRED',
        `outcome missing measured daemonEpoch/incarnation: ${JSON.stringify(outcome)}`,
      );
    }

    // Exact authenticated device identity returned by this pairing exchange.
    // Guessing via last active device is strictly prohibited.
    const deviceId = pairRes.host?.deviceId ?? null;
    if (!deviceId) {
      throw new TransportBarrier(
        'PAIRING_DEVICE_IDENTITY_SEAM_REQUIRED',
        'pairedHostPair response lacks exact pairing receipt deviceId; minimal explicit additive response field deviceId on PairedHostPairOk or HostView required; last-device guess forbidden',
      );
    }

    const paired = {
      backendSessionId,
      daemonEpoch: String(outcome.daemonEpoch),
      incarnation: String(outcome.incarnation),
      descriptor,
      target,
      hostId: host.hostId,
      deviceId,
      expectedRoute: 'relay',
      hostPid: hostChild.pid,
    };

    const fixture = {
      schema: TRANSPORT_PROVIDER_SCHEMA,
      root: clientProfile.root,
      profile: clientProfile,
      candidate: options.candidateStamp ?? 'candidate',
      binary,
      selection: { backendSessionId },
      paired,
    };

    const restartClientDaemon = async () => {
      if (clientChild && !clientChild.killed) {
        clientControl?.close();
        eventControl?.close();
        await terminateExactChild({
          child: clientChild,
          recorded: clientRecorded,
          probe,
          timeoutMs,
        });
      }

      clientRecorded = null;
      clientChild = null;

      try {
        // Re-spawn client daemon using the same client profile
        clientChild = spawn(binary, ['--daemon'], {
          cwd: repoRoot,
          env: clientProfile.env,
          stdio: ['ignore', 'pipe', 'pipe'],
          windowsHide: true,
        });
        const clientReady = waitForReady(clientChild, timeoutMs);
        clientReadyDisposer = clientReady.dispose;
        clientRecorded = await recordExactChild({
          child: clientChild,
          expectedBinary: binary,
          role: 'paired-client-daemon-restarted',
          probe,
          ledger,
        });
        await clientReady.promise;
        clientControl = await connectDaemonControl(daemonControlTransport(clientProfile), { timeoutMs });
        eventControl = await connectDaemonControl(daemonControlTransport(clientProfile), { timeoutMs });

        return {
          fixture,
          control: clientControl,
          eventControl,
          pairedOwner: pairedOwnerControls(clientControl, eventControl),
          getClientPid: async () => clientChild.pid,
          close: async () => {
            clientControl?.close();
            eventControl?.close();
          },
        };
      } catch (restartErr) {
        clientReadyDisposer?.();
        clientControl?.close();
        eventControl?.close();
        const cleanupErrors = [];
        let clientAlive = false;
        if (clientChild && clientRecorded) {
          try {
            await terminateExactChild({ child: clientChild, recorded: clientRecorded, probe, timeoutMs });
          } catch (cleanupErr) {
            cleanupErrors.push(cleanupErr);
            clientAlive = true;
          }
        } else if (clientChild) {
          clientAlive = true;
          cleanupErrors.push(new TransportBarrier(
            'OWNED_PROCESS_UNCONFIRMED_LEAK',
            `Client daemon PID ${clientChild.pid} spawned during restart but unconfirmed by probe; cannot safely signal`,
          ));
        }
        if (!clientAlive) {
          try { clientProfile.cleanup(); } catch {}
        }
        if (cleanupErrors.length > 0) {
          restartErr.cleanupErrors = cleanupErrors;
        }
        throw restartErr;
      }
    };

    const cleanup = async () => {
      clientControl?.close();
      eventControl?.close();
      hostControl?.close();
      const results = await Promise.allSettled([
        terminateExactChild({ child: clientChild, recorded: clientRecorded, probe, timeoutMs }),
        terminateExactChild({ child: hostChild, recorded: hostRecorded, probe, timeoutMs }),
      ]);
      const rejected = results.filter((r) => r.status === 'rejected');
      if (!results[1] || results[1].status === 'fulfilled') {
        try { hostProfile.cleanup(); } catch {}
      }
      if (!results[0] || results[0].status === 'fulfilled') {
        try { clientProfile.cleanup(); } catch {}
      }
      if (rejected.length > 0) {
        throw rejected[0].reason;
      }
    };

    return {
      fixture,
      paired,
      pid: clientChild.pid,
      hostPid: hostChild.pid,
      hostControl,
      clientControl,
      eventControl,
      pairedOwner: pairedOwnerControls(clientControl, eventControl),
      deliveryObserver: pairedHostDeliveryObserver(hostControl),
      restartClientDaemon,
      cleanup,
    };
  } catch (err) {
    hostReadyDisposer?.();
    clientReadyDisposer?.();
    clientControl?.close();
    eventControl?.close();
    hostControl?.close();
    const cleanupErrors = [];
    let hostAlive = false;
    let clientAlive = false;
    if (clientChild && clientRecorded) {
      try {
        await terminateExactChild({ child: clientChild, recorded: clientRecorded, probe, timeoutMs });
      } catch (cleanupErr) {
        cleanupErrors.push(cleanupErr);
        clientAlive = true;
      }
    } else if (clientChild) {
      clientAlive = true;
      cleanupErrors.push(new TransportBarrier(
        'OWNED_PROCESS_UNCONFIRMED_LEAK',
        `Client daemon PID ${clientChild.pid} spawned but unconfirmed by probe; cannot safely signal`,
      ));
    }
    if (hostChild && hostRecorded) {
      try {
        await terminateExactChild({ child: hostChild, recorded: hostRecorded, probe, timeoutMs });
      } catch (cleanupErr) {
        cleanupErrors.push(cleanupErr);
        hostAlive = true;
      }
    } else if (hostChild) {
      hostAlive = true;
      cleanupErrors.push(new TransportBarrier(
        'OWNED_PROCESS_UNCONFIRMED_LEAK',
        `Host daemon PID ${hostChild.pid} spawned but unconfirmed by probe; cannot safely signal`,
      ));
    }
    if (!hostAlive) {
      try { hostProfile.cleanup(); } catch {}
    }
    if (!clientAlive) {
      try { clientProfile.cleanup(); } catch {}
    }
    if (cleanupErrors.length > 0) {
      err.cleanupErrors = cleanupErrors;
    }
    throw err;
  }
}

/**
 * Provision an isolated paired session matching the provisionPairedFixture contract.
 */
export async function provisionIsolatedPairedSession(options = {}) {
  const harness = await launchPairedIsolatedHarness(options);
  return {
    paired: harness.paired,
    pid: harness.pid,
    pairedOwner: harness.pairedOwner,
    deliveryObserver: harness.deliveryObserver,
    harness,
  };
}