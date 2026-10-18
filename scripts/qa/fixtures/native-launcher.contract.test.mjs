import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { mkdir } from 'node:fs/promises';
import { watch } from 'node:fs';
import { buildSetupProbeSessionScript, buildWindowsChildWindowsScript, subscribeSetupProbe, runInteractiveSetupProbe, createRemoteFrontend, collectLaunchEvidence, inspectSetupTarget, inspectSetupChildWindows, launchNativeGui, launchNativeGuiAndRecordExit, persistLaunch, requestCorrelatedSnapshot, waitWindowsExactPidExit } from './native-remote-agent.mjs';

const SNAPSHOT_ID = '0b9f3f4e-7a51-4c2d-9e0f-3c1d2b4a5e6f';
const snapshotArgs = (profile) => ({ profile, runId: 'run', operationId: 'op', requestId: SNAPSHOT_ID,
  attachTuple: { backendSessionId: 'b', paneIdentity: 'p' }, timeoutMs: 9_000 });

// Stands in for the product's QA snapshot thread: answers only after the
// correlated request file lands, exactly as the GUI does.
async function answerSnapshotRequest(profile, respond) {
  const dir = join(profile, 'barriers');
  await new Promise((resolveAnswer, reject) => {
    const watcher = watch(dir, async (_, name) => {
      if (String(name) !== `native-snapshot-${SNAPSHOT_ID}.request.json`) return;
      try {
        const request = JSON.parse(await readFile(join(dir, String(name)), 'utf8'));
        watcher.close();
        const response = respond(request);
        if (response) await writeFile(join(dir, `native-snapshot-${SNAPSHOT_ID}.response.json`), JSON.stringify(response));
        resolveAnswer();
      } catch (error) { if (error.code !== 'ENOENT') { watcher.close(); reject(error); } }
    });
  });
}

test('snapshot request is correlated, clamped and returns the product response verbatim', async () => {
  const profile = await mkdtemp(join(tmpdir(), 'ferryx-snapshot-'));
  try {
    await mkdir(join(profile, 'barriers'));
    let seen;
    const answered = answerSnapshotRequest(profile, (request) => {
      seen = request;
      return { ...request, status: 'ok', producer: 'ipc-qa-barrier', producerPid: 7, snapshot: { attachTuple: request.attachTuple } };
    });
    const response = await requestCorrelatedSnapshot(snapshotArgs(profile));
    await answered;
    assert.deepEqual(seen, { runId: 'run', operationId: 'op', requestId: SNAPSHOT_ID,
      attachTuple: { backendSessionId: 'b', paneIdentity: 'p' }, timeoutMs: 5_000 });
    assert.equal(response.status, 'ok');
    assert.equal(response.requestId, SNAPSHOT_ID);
    assert.deepEqual(response.snapshot.attachTuple, seen.attachTuple);
    assert.deepEqual((await readdir(join(profile, 'barriers'))).filter((name) => name.includes('.tmp-')), []);
  } finally { await rm(profile, { recursive: true, force: true }); }
});

test('snapshot typed refusal passes through and a silent product fails with a typed timeout', async () => {
  const profile = await mkdtemp(join(tmpdir(), 'ferryx-snapshot-'));
  try {
    await mkdir(join(profile, 'barriers'));
    const answered = answerSnapshotRequest(profile, (request) => ({ ...request, status: 'refused', code: 'SNAPSHOT_TIMEOUT' }));
    const refused = await requestCorrelatedSnapshot(snapshotArgs(profile));
    await answered;
    assert.equal(refused.status, 'refused');
    assert.equal(refused.code, 'SNAPSHOT_TIMEOUT');
    await rm(join(profile, 'barriers'), { recursive: true });
    await mkdir(join(profile, 'barriers'));
    await assert.rejects(requestCorrelatedSnapshot({ ...snapshotArgs(profile), timeoutMs: 1 }),
      (error) => error.code === 'NATIVE_SNAPSHOT_TIMEOUT');
    await assert.rejects(requestCorrelatedSnapshot({ ...snapshotArgs(profile), requestId: 'not-a-uuid' }),
      (error) => error.code === 'NATIVE_SETUP_UNIMPLEMENTED');
  } finally { await rm(profile, { recursive: true, force: true }); }
});

test('interactive probe subscribes before scheduling and never guesses process ownership', async () => {
  const gui = { pid: 81, executable: 'C:\\ferryx.exe', processStart: '12345' };
  for (const mode of ['success', 'wrong-session', 'timeout', 'create-failure']) {
    const order = [];
    const files = new Map();
    const pendingTasks = new Map();
    const completion = Promise.withResolvers();
    completion.promise.catch(() => {});
    const failure = new Error('probe deadline');
    const stdout = JSON.stringify({ probe: 'setup-probe-session', targetPid: 81,
      probeSessionId: mode === 'wrong-session' ? 0 : 1, targetSessionId: 1 });
    const call = runInteractiveSetupProbe({ profile: '/profile', gui, script: 'READ_ONLY_PROBE', signal: new AbortController().signal }, {
      token: 'owned', executable: '/bun', runner: '/agent.mjs', pendingTasks,
      makeDirectory: async () => {}, saveScript: async (path, body) => { files.set(path, body); },
      persist: async (path, value) => { files.set(path, structuredClone(value)); },
      saveReport: async (path, value) => { files.set(path, structuredClone(value)); },
      subscribe: () => { order.push('subscribe'); return { promise: completion.promise, close: () => order.push('close') }; },
      invoke: async (exe, args) => {
        assert.equal(exe, 'schtasks');
        order.push(args[0]);
        assert.notEqual(args[0], '/end');
        if (args[0] === '/create') {
          assert.ok(args.includes('/it'));
          const action = args[args.indexOf('/tr') + 1];
          assert.equal(action, `wscript.exe //B //Nologo "${join('/profile', 'evidence', 'setup-probes', 'owned', 'hidden-probe.vbs')}"`);
          assert.doesNotMatch(action, /cmd\.exe|bun(?:\.exe)?\s/i);
          const wrapper = files.get(join('/profile', 'evidence', 'setup-probes', 'owned', 'hidden-probe.vbs'));
          assert.match(wrapper, /CreateObject\("WScript.Shell"\)/);
          assert.ok(wrapper.includes(', 0, True)'));
          assert.ok(wrapper.includes('cmd.exe /d /s /c """"'));
          assert.doesNotMatch(wrapper, /SetForegroundWindow|SetFocus|AppActivate|SendKeys|taskkill/i);
          if (mode === 'create-failure') throw failure;
        }
        if (args[0] === '/run') {
          if (mode === 'timeout') completion.reject(failure);
          else completion.resolve({ owner: { pid: 91, processStart: 'own-start', executable: '/bun' }, stdout });
        }
        return '';
      },
    });
    if (mode === 'success') assert.equal(await call, stdout);
    else await assert.rejects(call, mode === 'wrong-session' ? /target GUI session/ : error => error === failure);
    assert.equal(order[0], 'subscribe');
    assert.deepEqual(order, mode === 'create-failure' ? ['subscribe', '/create', 'close']
      : mode === 'timeout' ? ['subscribe', '/create', '/run', 'close']
      : ['subscribe', '/create', '/run', '/delete', 'close']);
    const report = files.get(join('/profile', 'evidence', 'setup-probe-dispatch.json'));
    assert.equal(report.processKillAttempted, false);
    assert.equal(pendingTasks.has('/profile'), mode === 'timeout');
    assert.equal(files.get(join('/profile', 'evidence', 'setup-probes', 'owned', 'probe.json')).script, 'READ_ONLY_PROBE');
  }
  const script = buildSetupProbeSessionScript(gui);
  assert.match(script, /Get-Process -Id 81/);
  assert.match(script, /StartTime.ToUniversalTime\(\).Ticks/);
  assert.match(script, /targetSessionId = \$target.SessionId/);
  assert.match(script, /probeSessionId = \$probeSession/);
  assert.doesNotMatch(script, /taskkill|SetFocus|SetForegroundWindow|InvokePattern/);
});

test('interactive probe retains timed-out task when a subsequent probe on the same profile succeeds', async () => {
  const gui = { pid: 81, executable: 'C:\\ferryx.exe', processStart: '12345' };
  const pendingTasks = new Map();
  const profile = '/profile';
  const stdout = JSON.stringify({ probe: 'setup-probe-session', targetPid: 81,
    probeSessionId: 1, targetSessionId: 1 });
  const timeoutFailure = new Error('probe deadline');

  // Probe 1 times out: task must be retained
  const completion1 = Promise.withResolvers();
  completion1.promise.catch(() => {});
  const probe1 = runInteractiveSetupProbe({ profile, gui, script: 'PROBE_1', signal: new AbortController().signal }, {
    token: 'token-1', executable: '/bun', runner: '/agent.mjs', pendingTasks,
    makeDirectory: async () => {}, saveScript: async () => {}, persist: async () => {}, saveReport: async () => {},
    subscribe: () => ({ promise: completion1.promise, close: () => {} }),
    invoke: async (exe, args) => {
      if (args[0] === '/run') completion1.reject(timeoutFailure);
      return '';
    },
  });
  await assert.rejects(probe1, error => error === timeoutFailure);
  assert.equal(pendingTasks.has(profile), true);
  assert.ok(pendingTasks.get(profile) instanceof Set);
  assert.equal(pendingTasks.get(profile).has('ferryx-setup-token-1'), true);
  assert.equal(pendingTasks.get(profile).size, 1);

  // Probe 2 on the same profile succeeds: must not erase or overwrite probe 1's retained task
  const completion2 = Promise.withResolvers();
  completion2.promise.catch(() => {});
  const deletedTasks = [];
  const probe2 = runInteractiveSetupProbe({ profile, gui, script: 'PROBE_2', signal: new AbortController().signal }, {
    token: 'token-2', executable: '/bun', runner: '/agent.mjs', pendingTasks,
    makeDirectory: async () => {}, saveScript: async () => {}, persist: async () => {}, saveReport: async () => {},
    subscribe: () => ({ promise: completion2.promise, close: () => {} }),
    invoke: async (exe, args) => {
      if (args[0] === '/run') completion2.resolve({ owner: { pid: 92, processStart: 'start-2', executable: '/bun' }, stdout });
      if (args[0] === '/delete') deletedTasks.push(args[args.indexOf('/tn') + 1]);
      return '';
    },
  });
  assert.equal(await probe2, stdout);
  assert.deepEqual(deletedTasks, ['ferryx-setup-token-2']);

  // Earlier task retention is preserved; only the successful probe's task was removed
  assert.equal(pendingTasks.has(profile), true);
  assert.equal(pendingTasks.get(profile).has('ferryx-setup-token-1'), true);
  assert.equal(pendingTasks.get(profile).has('ferryx-setup-token-2'), false);
  assert.equal(pendingTasks.get(profile).size, 1);
});

test('interactive probe deletes profile entry from pending tasks upon successful probe completion', async () => {
  const gui = { pid: 81, executable: 'C:\\ferryx.exe', processStart: '12345' };
  const pendingTasks = new Map();
  const profile = '/profile-fresh';
  const stdout = JSON.stringify({ probe: 'setup-probe-session', targetPid: 81,
    probeSessionId: 1, targetSessionId: 1 });
  const completion = Promise.withResolvers();
  completion.promise.catch(() => {});
  const deletedTasks = [];
  const call = runInteractiveSetupProbe({ profile, gui, script: 'PROBE_FRESH', signal: new AbortController().signal }, {
    token: 'fresh-1', executable: '/bun', runner: '/agent.mjs', pendingTasks,
    makeDirectory: async () => {}, saveScript: async () => {}, persist: async () => {}, saveReport: async () => {},
    subscribe: () => ({ promise: completion.promise, close: () => {} }),
    invoke: async (exe, args) => {
      if (args[0] === '/run') completion.resolve({ owner: { pid: 94, processStart: 'start-fresh', executable: '/bun' }, stdout });
      if (args[0] === '/delete') deletedTasks.push(args[args.indexOf('/tn') + 1]);
      return '';
    },
  });
  assert.equal(await call, stdout);
  assert.deepEqual(deletedTasks, ['ferryx-setup-fresh-1']);
  assert.equal(pendingTasks.has(profile), false);
  assert.equal(pendingTasks.get(profile), undefined);
});

test('setup discovery validates persisted launch provenance and exact live identity without input', async () => {
  const profile = '/isolated/profile';
  const gui = { pid: 81, role: 'gui', status: 'spawned', executable: '/isolated/ferryx', processStart: '81:1',
    runId: 'run', operationId: 'op', candidate: 'candidate', hostId: 'host' };
  const spec = { ...gui, binary: gui.executable };
  const receipts = new Map();
  const scripts = [];
  let probes = 0;
  const deps = { platform: 'win32', ownedProfiles: new Set([profile]), receipts,
    save: async () => {},
    load: async path => path.endsWith('gui-launch-spec.json') ? spec : gui,
    inspect: async () => { probes++; return { ...gui, exited: false }; },
    run: async script => {
      scripts.push(script);
      assert.doesNotMatch(script, /\.Invoke\(|SetFocus|SetForegroundWindow|SendKeys/);
      return JSON.stringify(scripts.length === 1
        ? { probe: 'owned-windows', pid: gui.pid, windows: [{ hwnd: 19, visible: true }], mainWindowHandle: 19 }
        : { probe: 'new-pane', pid: gui.pid, result: 'PANE_TARGET', actionableCount: 1,
          chosen: { windowHwnd: 19, name: 'New Terminal', enabled: true, offscreen: false, rectEmpty: false, rect: '10,20,60,80' } });
    } };
  const request = { profile, gui, signal: new AbortController().signal };
  assert.deepEqual(await inspectSetupTarget(request, deps), { gui, hwnd: 19, point: { x: 40, y: 60 } });
  assert.equal(probes, 2);
  assert.equal(Object.isFrozen(receipts.get(profile)), true);
  await assert.rejects(inspectSetupTarget({ ...request, profile: '/foreign' }, deps), /owned Windows profile/);
  await assert.rejects(inspectSetupTarget({ ...request, gui: { ...gui, processStart: 'replaced' } }, deps), /immutable launch receipt/);
  await assert.rejects(inspectSetupTarget(request, { ...deps, receipts: new Map(),
    load: async path => path.endsWith('gui-launch-spec.json') ? spec : { ...gui, operationId: 'foreign' },
  }), /provisioned launch/);
  await assert.rejects(inspectSetupTarget(request, { ...deps,
    inspect: async () => ({ ...gui, exited: false, processStart: 'replaced' }),
  }), /exact live identity/);
  assert.equal(scripts.length, 2);
});

test('setup refusal exports raw probes, candidates and exact rejection conditions from the owned profile', async () => {
  const profile = '/isolated/profile';
  const gui = { pid: 81, executable: '/isolated/ferryx', processStart: '81:1' };
  const signal = new AbortController().signal;
  for (const mode of ['enumeration', 'name', 'offscreen', 'missing-window']) {
    const enumerationRaw = mode === 'enumeration' ? 'not JSON\r\n' : JSON.stringify({
      probe: 'owned-windows', pid: 81, windows: [{ hwnd: 19, visible: true }], mainWindowHandle: 19 });
    const chosen = { windowHwnd: mode === 'missing-window' ? 99 : 19,
      name: mode === 'name' ? 'Other' : 'New Terminal', enabled: true,
      offscreen: mode === 'offscreen', rectEmpty: false, rect: '10,20,60,80' };
    const newPane = { probe: 'new-pane', pid: 81, result: 'PANE_TARGET', actionableCount: 1, candidates: [chosen], chosen };
    const newPaneRaw = `probe output\r\n${JSON.stringify(newPane)}\r\n`;
    let saved;
    let calls = 0;
    const deps = { platform: 'win32', ownedProfiles: new Set([profile]), receipts: new Map([[profile, Object.freeze(gui)]]),
      load: async () => ({}), inspect: async () => ({ ...gui, exited: false }),
      run: async () => ++calls === 1 ? enumerationRaw : newPaneRaw,
      save: async (path, diagnostics) => { assert.equal(path, join(profile, 'evidence', 'setup-target.json')); saved = diagnostics; },
    };
    await assert.rejects(inspectSetupTarget({ profile, gui, signal }, deps), /Setup (owned-window enumeration failed|New Terminal target refused)/);
    assert.equal(saved.enumerationRaw, enumerationRaw);
    const expected = { enumeration: 'enumeration-unreadable', name: 'chosen-name-mismatch',
      offscreen: 'chosen-not-onscreen', 'missing-window': 'chosen-window-not-owned' }[mode];
    assert.deepEqual(saved.rejections, [expected]);
    if (mode !== 'enumeration') {
      assert.equal(saved.newPaneRaw, newPaneRaw);
      assert.deepEqual(saved.newPane.probe.candidates, [chosen]);
    }
    const bytes = Buffer.from(`${JSON.stringify(saved)}\n`);
    const files = await collectLaunchEvidence(profile, signal, { ownedProfiles: new Set([profile]), read: async path => {
      if (path === join(profile, 'evidence', 'setup-target.json')) return bytes;
      throw Object.assign(new Error('absent'), { code: 'ENOENT' });
    } });
    assert.equal(files.find(file => file.name === 'evidence/setup-target.json').base64, bytes.toString('base64'));
    saved = undefined;
    await assert.rejects(inspectSetupTarget({ profile: '/foreign', gui, signal }, deps), /owned Windows profile/);
    assert.equal(saved, undefined);
  }
});

test('setup hit-test refuses foreign foreground and an occluding window', async () => {
  const profile = '/isolated/profile';
  const gui = { pid: 81, executable: '/isolated/ferryx', processStart: '81:1' };
  for (const [foregroundPid, foregroundHwnd, hitHwnd, owned] of [[81, 19, 19, true], [99, 19, 19, false], [81, 20, 19, false], [81, 19, 99, false]]) {
    const result = await inspectSetupTarget({ profile, gui, hwnd: 19, point: { x: 40, y: 60 }, signal: new AbortController().signal }, {
      platform: 'win32', ownedProfiles: new Set([profile]), receipts: new Map([[profile, Object.freeze(gui)]]), load: async () => ({}),
      inspect: async () => ({ ...gui, exited: false }),
      run: async script => {
        assert.match(script, /WindowFromPoint/);
        assert.doesNotMatch(script, /SetForegroundWindow|SetFocus|\.Invoke\(/);
        return JSON.stringify({ probe: 'setup-hit', foregroundPid, foregroundHwnd, hitHwnd });
      },
    });
    assert.equal(result.owned, owned);
  }
});

test('setup child diagnostic resolves registered GUI receipt by profile and enumerates all owned visible top-level children', async () => {
  const profile = '/isolated/profile';
  const gui = { pid: 81, role: 'gui', status: 'spawned', executable: '/isolated/ferryx', processStart: '81:1',
    runId: 'run', operationId: 'op', candidate: 'candidate', hostId: 'host' };
  const spec = { ...gui, binary: gui.executable };
  const signal = new AbortController().signal;

  let savedDiagnostics;
  let executedScript = '';
  const deps = {
    platform: 'win32',
    ownedProfiles: new Set([profile]),
    receipts: new Map([[profile, Object.freeze(gui)]]),
    load: async path => path.endsWith('gui-launch-spec.json') ? spec : gui,
    inspect: async () => ({ ...gui, exited: false }),
    run: async (script, targetGui, probeSignal) => {
      executedScript = script;
      assert.equal(targetGui.pid, gui.pid);
      assert.ok(probeSignal);
      assert.match(script, /EnumChildWindows/);
      assert.match(script, /GetWindowRect/);
      assert.match(script, /IsWindowVisible/);
      assert.match(script, /GetClassNameW/);
      assert.match(script, /GetWindowThreadProcessId/);
      assert.doesNotMatch(script, /MainWindowHandle/);
      assert.doesNotMatch(script, /SetFocus|SetForegroundWindow|SendKeys|\.Invoke/);
      return JSON.stringify({
        probe: 'setup-child-windows',
        pid: gui.pid,
        visibleTopHwnds: [19, 21],
        children: [
          { hwnd: 101, topHwnd: 19, ownerPid: 81, className: 'FerryxNativeTerm', visible: false, rect: { left: 10, top: 10, right: 810, bottom: 610, width: 800, height: 600 } },
          { hwnd: 102, topHwnd: 19, ownerPid: 120, className: 'Chrome_RenderWidgetHostHWND', visible: true, rect: { left: 0, top: 0, right: 1000, bottom: 800, width: 1000, height: 800 } },
        ],
      });
    },
    save: async (path, diag) => {
      assert.equal(path, join(profile, 'evidence', 'setup-child-windows.json'));
      savedDiagnostics = diag;
    },
  };

  // Profile-only invocation: resolves registered GUI receipt without caller-invented PID
  const result = await inspectSetupChildWindows({ profile, signal }, deps);
  assert.equal(result.gui.pid, 81);
  assert.deepEqual(result.visibleTopHwnds, [19, 21]);
  assert.equal(result.childCount, 2);
  assert.equal(result.children.length, 2);
  assert.equal(result.children[0].className, 'FerryxNativeTerm');
  assert.equal(result.children[0].topHwnd, 19);
  assert.equal(result.children[0].ownerPid, 81);
  assert.equal(result.children[0].visible, false);
  assert.deepEqual(result.children[0].rect, { left: 10, top: 10, right: 810, bottom: 610, width: 800, height: 600 });
  assert.equal(result.children[1].className, 'Chrome_RenderWidgetHostHWND');
  assert.equal(result.children[1].topHwnd, 19);
  assert.equal(result.children[1].ownerPid, 120);
  assert.equal(result.children[1].visible, true);
  assert.equal(savedDiagnostics.childCount, 2);

  // Absence of children returns childCount 0 and empty array; does NOT throw or claim failure
  const depsEmpty = {
    ...deps,
    run: async () => JSON.stringify({ probe: 'setup-child-windows', pid: gui.pid, visibleTopHwnds: [19], children: [] }),
  };
  const resultEmpty = await inspectSetupChildWindows({ profile, signal }, depsEmpty);
  assert.equal(resultEmpty.childCount, 0);
  assert.deepEqual(resultEmpty.children, []);

  // Unowned profile throws typed error
  await assert.rejects(inspectSetupChildWindows({ profile: '/foreign', signal }, deps), /owned Windows profile/);
});

test('setup child diagnostic rejects PID mismatch, malformed children and invalid entries', async () => {
  const profile = '/isolated/profile';
  const gui = { pid: 81, role: 'gui', status: 'spawned', executable: '/isolated/ferryx', processStart: '81:1',
    runId: 'run', operationId: 'op', candidate: 'candidate', hostId: 'host' };
  const spec = { ...gui, binary: gui.executable };
  const signal = new AbortController().signal;
  const baseDeps = {
    platform: 'win32',
    ownedProfiles: new Set([profile]),
    receipts: new Map([[profile, Object.freeze(gui)]]),
    load: async path => path.endsWith('gui-launch-spec.json') ? spec : gui,
    inspect: async () => ({ ...gui, exited: false }),
    save: async () => {},
  };

  // 1. PID mismatch rejection
  await assert.rejects(inspectSetupChildWindows({ profile, signal }, {
    ...baseDeps,
    run: async () => JSON.stringify({ probe: 'setup-child-windows', pid: 999, visibleTopHwnds: [19], children: [] }),
  }), /unreadable or PID mismatch/);

  // 2. Non-array children rejection
  await assert.rejects(inspectSetupChildWindows({ profile, signal }, {
    ...baseDeps,
    run: async () => JSON.stringify({ probe: 'setup-child-windows', pid: gui.pid, visibleTopHwnds: [19], children: 'not-an-array' }),
  }), /children malformed/);

  // 3. Malformed child entry rejection (non-positive hwnd)
  await assert.rejects(inspectSetupChildWindows({ profile, signal }, {
    ...baseDeps,
    run: async () => JSON.stringify({
      probe: 'setup-child-windows', pid: gui.pid, visibleTopHwnds: [19],
      children: [{ hwnd: 0, topHwnd: 19, ownerPid: 81, className: 'FerryxNativeTerm', visible: false, rect: {} }],
    }),
  }), /entry malformed/);

  // 4. Malformed child entry rejection (missing/non-positive topHwnd)
  await assert.rejects(inspectSetupChildWindows({ profile, signal }, {
    ...baseDeps,
    run: async () => JSON.stringify({
      probe: 'setup-child-windows', pid: gui.pid, visibleTopHwnds: [19],
      children: [{ hwnd: 101, topHwnd: 0, ownerPid: 81, className: 'FerryxNativeTerm', visible: false, rect: {} }],
    }),
  }), /entry malformed/);

  // 5. Malformed child entry rejection (missing/invalid ownerPid)
  await assert.rejects(inspectSetupChildWindows({ profile, signal }, {
    ...baseDeps,
    run: async () => JSON.stringify({
      probe: 'setup-child-windows', pid: gui.pid, visibleTopHwnds: [19],
      children: [{ hwnd: 101, topHwnd: 19, ownerPid: 'invalid', className: 'FerryxNativeTerm', visible: false, rect: {} }],
    }),
  }), /entry malformed/);

  // 6. Unreadable probe output
  await assert.rejects(inspectSetupChildWindows({ profile, signal }, {
    ...baseDeps,
    run: async () => 'not json\r\n',
  }), /enumeration unreadable/);

  // 7. Malformed visibleTopHwnds (not an array)
  await assert.rejects(inspectSetupChildWindows({ profile, signal }, {
    ...baseDeps,
    run: async () => JSON.stringify({
      probe: 'setup-child-windows', pid: gui.pid, visibleTopHwnds: 'not-an-array', children: [],
    }),
  }), /visibleTopHwnds malformed/);

  // 8. Malformed visibleTopHwnds (contains zero or non-positive)
  await assert.rejects(inspectSetupChildWindows({ profile, signal }, {
    ...baseDeps,
    run: async () => JSON.stringify({
      probe: 'setup-child-windows', pid: gui.pid, visibleTopHwnds: [0], children: [],
    }),
  }), /visibleTopHwnds malformed/);

  // 9. Child topHwnd foreign to visibleTopHwnds
  await assert.rejects(inspectSetupChildWindows({ profile, signal }, {
    ...baseDeps,
    run: async () => JSON.stringify({
      probe: 'setup-child-windows', pid: gui.pid, visibleTopHwnds: [19, 21],
      children: [{ hwnd: 101, topHwnd: 999, ownerPid: 81, className: 'FerryxNativeTerm', visible: false, rect: { left: 0, top: 0, right: 10, bottom: 10, width: 10, height: 10 } }],
    }),
  }), /entry malformed/);
});

test('subscribeSetupProbe requires both JSON receipt and wrapper marker with successful status before resolution', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'probe-barrier-'));
  const controller = new AbortController();
  try {
    const completionPath = join(dir, 'completion.json');
    const markerPath = join(dir, 'wrapper-exit.marker');
    const subscription = subscribeSetupProbe({ completionPath, markerPath }, controller.signal, 10000);
    let resolved = false;
    subscription.promise.then(() => { resolved = true; });
    await writeFile(completionPath, JSON.stringify({ owner: { pid: 81 }, stdout: 'done' }));
    await new Promise(r => setImmediate(r));
    assert.equal(resolved, false);
    await writeFile(markerPath, '0\n');
    const result = await subscription.promise;
    assert.equal(result.stdout, 'done');
    assert.equal(result.wrapperExitCode, 0);
  } finally {
    controller.abort();
    await rm(dir, { recursive: true, force: true });
  }
});

test('interactive probe rejects on nonzero wrapper marker and probe retains task without deletion', async () => {
  const profile = '/profile-nonzero';
  const gui = { pid: 81, executable: 'C:\\ferryx.exe', processStart: '12345' };
  const pendingTasks = new Map();
  const deleted = [];
  const call = runInteractiveSetupProbe({ profile, gui, script: 'PROBE', signal: new AbortController().signal }, {
    token: 'nz-token', executable: '/bun', runner: '/agent.mjs', pendingTasks,
    makeDirectory: async () => {}, saveScript: async () => {}, persist: async () => {}, saveReport: async () => {},
    subscribe: () => ({
      promise: Promise.reject(new Error('Interactive setup probe wrapper exited with code 1; task retained, no process killed')),
      close: () => {},
    }),
    invoke: async (exe, args) => {
      if (args[0] === '/delete') deleted.push(args);
      return '';
    },
  });
  await assert.rejects(call, /wrapper exited with code 1; task retained, no process killed/);
  assert.deepEqual(deleted, []);
  assert.equal(pendingTasks.has(profile), true);
  assert.equal(pendingTasks.get(profile).has('ferryx-setup-nz-token'), true);
});

test('interactive probe cancels immediately on abort signal and retains task without deletion', async () => {
  const profile = '/profile-cancel';
  const gui = { pid: 81, executable: 'C:\\ferryx.exe', processStart: '12345' };
  const pendingTasks = new Map();
  const deleted = [];
  const controller = new AbortController();
  const cancelReason = new Error('test-abort-reason');
  const call = runInteractiveSetupProbe({ profile, gui, script: 'PROBE', signal: controller.signal }, {
    token: 'cancel-token', executable: '/bun', runner: '/agent.mjs', pendingTasks,
    makeDirectory: async () => {}, saveScript: async () => {}, persist: async () => {}, saveReport: async () => {},
    subscribe: () => ({
      promise: new Promise((_, reject) => {
        controller.signal.addEventListener('abort', () => reject(controller.signal.reason), { once: true });
      }),
      close: () => {},
    }),
    invoke: async (exe, args) => {
      if (args[0] === '/run') controller.abort(cancelReason);
      if (args[0] === '/delete') deleted.push(args);
      return '';
    },
  });
  await assert.rejects(call, error => error === cancelReason);
  assert.deepEqual(deleted, []);
  assert.equal(pendingTasks.has(profile), true);
  assert.equal(pendingTasks.get(profile).has('ferryx-setup-cancel-token'), true);
});

test('remote frontend uses the helper registry and strips server handles from readiness receipts', async () => {
  let closes = 0;
  const server = { close(callback) { closes++; callback(); }, closeAllConnections() {} };
  const resource = createRemoteFrontend({ serve: async options => {
    assert.equal(options.rootDir, '/checkout');
    assert.equal(options.distDir, '/checkout/ui/dist');
    options.registry.registerServer(server, 'frontend-dist');
    return { server, action: 'frontend.served', url: 'http://127.0.0.1:5173/', route: 'static-ui-dist' };
  } });
  const receipt = await resource.start({ worktree: '/checkout', uiDist: '/checkout/ui/dist',
    frontend: { mode: 'debug', url: 'http://127.0.0.1:5173' }, signal: new AbortController().signal });
  assert.deepEqual(receipt, { action: 'frontend.served', url: 'http://127.0.0.1:5173/', route: 'static-ui-dist' });
  assert.equal(closes, 0);
  await resource.close();
  await resource.close();
  assert.equal(closes, 1);
});

test('remote frontend refuses implicit modes and URL drift before touching the helper', async () => {
  let calls = 0;
  for (const frontend of [undefined, { mode: 'release' }, { mode: 'debug', url: 'http://127.0.0.1:5174' }]) {
    const resource = createRemoteFrontend({ serve: async () => { calls++; } });
    await assert.rejects(resource.start({ frontend, signal: new AbortController().signal }), /Explicit frontend/);
  }
  assert.equal(calls, 0);
});

test('remote frontend preserves helper failures and closes only registered owned servers', async () => {
  for (const registered of [false, true]) {
    const error = Object.assign(new Error('helper failed'), {
      code: registered ? 'FRONTEND_NOT_SERVED' : 'FRONTEND_PORT_OCCUPIED',
    });
    let closes = 0;
    const resource = createRemoteFrontend({ serve: async ({ registry }) => {
      if (registered) registry.registerServer({ close(callback) { closes++; callback(); } });
      throw error;
    } });
    await assert.rejects(resource.start({ frontend: { mode: 'debug', url: 'http://127.0.0.1:5173' },
      signal: new AbortController().signal }), actual => actual === error);
    assert.equal(closes, registered ? 1 : 0);
  }
});

test('remote frontend cancellation before registration closes a late owned listener and never reports ready', async () => {
  const controller = new AbortController();
  const entered = Promise.withResolvers();
  const resume = Promise.withResolvers();
  const error = new Error('setup cancelled');
  let closes = 0;
  const resource = createRemoteFrontend({ serve: async ({ registry }) => {
    entered.resolve();
    await resume.promise;
    registry.registerServer({ close(callback) { closes++; callback(); } });
    return { action: 'frontend.served' };
  } });
  const pending = resource.start({ frontend: { mode: 'debug', url: 'http://127.0.0.1:5173' }, signal: controller.signal });
  const rejected = assert.rejects(pending, actual => actual === error);
  await entered.promise;
  controller.abort(error);
  resume.resolve();
  await rejected;
  await resource.close();
  assert.equal(closes, 1);
});

test('launch evidence collection reads only fixed owned paths and preserves original bytes', async () => {
  const profile = '/isolated/profile';
  const ownedProfiles = new Set([profile]);
  const signal = new AbortController().signal;
  const bytes = Buffer.from([0, 255, 13, 10, 32]);
  const names = ['receipts/gui-launch.json', 'receipts/gui-launch.json.exit.json',
    'evidence/launcher-entry.log', 'evidence/launcher-output.log', 'evidence/launcher-exit.log',
    'evidence/gui-stdout.log', 'evidence/gui-stderr.log', 'runtime/boot-trace.log', 'evidence/setup-target.json',
    'evidence/setup-probe-dispatch.json', 'evidence/setup-child-windows.json', 'tmp/ferryx-switch-debug.jsonl'];
  const reads = [];
  const files = await collectLaunchEvidence(profile, signal, { ownedProfiles, read: async (path, options) => {
    reads.push(path);
    assert.equal(options.signal, signal);
    if (path === join(profile, names[1])) throw Object.assign(new Error('absent'), { code: 'ENOENT' });
    return bytes;
  } });
  assert.deepEqual(reads, names.map(name => join(profile, name)));
  assert.deepEqual(files, names.map((name, index) => index === 1 ? { name, missing: true }
    : { name, base64: bytes.toString('base64') }));
  assert.equal(ownedProfiles.has(profile), true);
});

test('launch evidence collection allows an absent boot trace without fabricating bytes', async () => {
  const profile = '/isolated/profile';
  const files = await collectLaunchEvidence(profile, new AbortController().signal, {
    ownedProfiles: new Set([profile]), read: async path => {
      if (path === join(profile, 'runtime/boot-trace.log')) {
        throw Object.assign(new Error('absent'), { code: 'ENOENT' });
      }
      return Buffer.from([0, 255, 13, 10, 32]);
    },
  });
  assert.deepEqual(files.find(file => file.name === 'runtime/boot-trace.log'),
    { name: 'runtime/boot-trace.log', missing: true });
});

test('launch evidence collection rejects unowned profiles and propagates non-absence errors and cancellation', async () => {
  const profile = '/isolated/profile';
  let reads = 0;
  await assert.rejects(collectLaunchEvidence(profile, new AbortController().signal, {
    ownedProfiles: new Set(), read: async () => { reads++; },
  }), /Unowned launch evidence/);
  assert.equal(reads, 0);
  const error = Object.assign(new Error('denied'), { code: 'EACCES' });
  await assert.rejects(collectLaunchEvidence(profile, new AbortController().signal, {
    ownedProfiles: new Set([profile]), read: async () => { throw error; },
  }), actual => actual === error);
  const controller = new AbortController();
  controller.abort(error);
  await assert.rejects(collectLaunchEvidence(profile, controller.signal, {
    ownedProfiles: new Set([profile]), read: async () => { reads++; },
  }), actual => actual === error);
  assert.equal(reads, 0);
});

test('Windows exit subscription scopes missing-PID handling to lookup and checks incarnation before wait', async () => {
  const signal = new AbortController().signal;
  let calls = 0;
  await waitWindowsExactPidExit(23452, '639278447206325549', signal, async (executable, args, receivedSignal) => {
    calls++;
    assert.equal(executable, 'powershell.exe');
    assert.equal(receivedSignal, signal);
    assert.deepEqual(args.slice(0, 3), ['-NoProfile', '-NonInteractive', '-Command']);
    const script = args[3];
    assert.ok(script.startsWith('try {$p=Get-Process -Id 23452 -ErrorAction Stop} catch {'));
    assert.ok(script.includes("$_.FullyQualifiedErrorId -eq 'NoProcessFoundForGivenId,Microsoft.PowerShell.Commands.GetProcessCommand' -and $_.CategoryInfo.Category -eq [System.Management.Automation.ErrorCategory]::ObjectNotFound"));
    assert.ok(script.includes("{exit 0}; throw}; if($p.StartTime.ToUniversalTime().Ticks.ToString() -ne '639278447206325549'){throw 'PID incarnation mismatch'}; $p.WaitForExit()"));
  });
  assert.equal(calls, 1);
});

test('Windows exit subscription propagates reused PID and all other runner failures', async () => {
  for (const message of ['PID incarnation mismatch', 'access denied', 'unreadable start time', 'wait failed']) {
    const error = new Error(message);
    await assert.rejects(waitWindowsExactPidExit(23452, '639278447206325549',
      new AbortController().signal, async () => { throw error; }), actual => actual === error);
  }
});

test('Windows exit subscription preserves cancellation before and during the runner', async () => {
  for (const before of [true, false]) {
    const controller = new AbortController();
    const error = new Error('cancelled');
    let calls = 0;
    if (before) controller.abort(error);
    await assert.rejects(waitWindowsExactPidExit(23452, '639278447206325549', controller.signal, async () => {
      calls++;
      controller.abort(error);
    }), actual => actual === error);
    assert.equal(calls, before ? 0 : 1);
  }
});

function fixture() {
  const profile = '/checkout/worktrees/recovery/.native-qa/run-1';
  const spec = { worktreeRoot: '/checkout/worktrees', worktree: '/checkout/worktrees/recovery', profile,
    binary: '/checkout/worktrees/recovery/bin/ferryx', binarySha256: 'hash', runId: 'run-1',
    operationId: 'op-1', candidate: 'candidate-1', hostId: 'host-1', receiptPath: `${profile}/receipts/gui-launch.json`,
    env: { FERRYX_QA_ISOLATED: '1', FERRYX_DATA_DIR: `${profile}/data`, FERRYX_RUNTIME_DIR: `${profile}/runtime`,
      FERRYX_SESSION_DIR: `${profile}/session`, FERRYX_QA_BARRIER_DIR: `${profile}/barriers`,
      FERRYX_QA_RUN_ID: 'run-1', FERRYX_QA_OPERATION_ID: 'op-1', HOME: `${profile}/home`, USERPROFILE: `${profile}/home` } };
  const child = Object.assign(new EventEmitter(), { pid: 71 });
  const killed = [];
  child.kill = (signal) => { killed.push({ pid: child.pid, signal }); return true; };
  const identity = { executable: spec.binary, processStart: 'start-71', exited: false };
  const calls = [];
  const deps = { realpath: async (path) => path, hashFile: async () => 'hash', validateWorktree: async () => {},
    open: async () => ({ fd: 81, close: async () => {} }),
    spawn: () => { calls.push('spawn'); return child; },
    probe: async (pid) => { assert.equal(pid, 71); assert.equal(child.listenerCount('exit'), 1); return identity; },
    persist: async (path, raw) => { calls.push('persist'); assert.equal(path, spec.receiptPath); assert.equal(raw.pid, 71); } };
  return { spec, child, killed, identity, calls, deps };
}

test('GUI stdout and stderr use exclusive owned files and dispose launcher handles after acceptance', async () => {
  const f = fixture();
  const closed = [];
  const opened = [];
  f.deps.open = async (path, flags, mode) => {
    const fd = 81 + opened.length;
    opened.push({ path, flags, mode });
    return { fd, close: async () => { closed.push(fd); } };
  };
  f.deps.spawn = (binary, args, options) => {
    assert.equal(binary, f.spec.binary);
    assert.deepEqual(args, []);
    assert.deepEqual(options.stdio, ['ignore', 81, 82]);
    assert.equal(options.cwd, f.spec.worktree);
    assert.deepEqual(closed, []);
    return f.child;
  };
  const result = await launchNativeGui(f.spec, f.deps);
  assert.deepEqual(opened, ['gui-stdout.log', 'gui-stderr.log'].map(name => ({
    path: join(f.spec.profile, 'evidence', name), flags: 'wx', mode: 0o600,
  })));
  assert.deepEqual(closed, [81, 82]);
  assert.equal(result.receipt.processStart, f.identity.processStart);
  assert.deepEqual(f.killed, []);
});

test('GUI log handles close on partial open, synchronous spawn failure and refused identity', async () => {
  for (const stage of ['open', 'spawn', 'identity']) {
    const f = fixture();
    const closed = [];
    let opens = 0;
    const error = new Error(`${stage} rejected`);
    f.deps.open = async () => {
      if (++opens === 2 && stage === 'open') throw error;
      const fd = opens;
      return { fd, close: async () => { closed.push(fd); } };
    };
    if (stage === 'spawn') f.deps.spawn = () => { throw error; };
    if (stage === 'identity') f.deps.probe = async () => { throw error; };
    await assert.rejects(launchNativeGui(f.spec, f.deps), actual => actual === error);
    assert.deepEqual(closed, stage === 'open' ? [1] : [1, 2]);
    assert.deepEqual(f.killed, []);
  }
});

test('GUI entry records owned nonzero and signal exits separately from successful launch', async () => {
  for (const outcome of [{ code: 7, signal: null }, { code: null, signal: 'SIGTERM' }]) {
    const f = fixture();
    const launched = Promise.withResolvers();
    const records = [];
    const pending = launchNativeGuiAndRecordExit(f.spec, {
      launch: async spec => {
        const result = await launchNativeGui(spec, f.deps);
        launched.resolve(result.receipt);
        return result;
      },
      persist: async (path, receipt) => { records.push({ path, receipt }); },
    });
    const receipt = await launched.promise;
    assert.equal(receipt.status, 'spawned');
    assert.deepEqual(records, []);
    f.child.emit('exit', outcome.code, outcome.signal);
    const expected = { ...receipt, status: 'exited', ...outcome };
    assert.deepEqual(await pending, expected);
    assert.deepEqual(records, [{ path: `${f.spec.receiptPath}.exit.json`, receipt: expected }]);
    assert.equal(receipt.status, 'spawned');
    assert.deepEqual(f.killed, []);
  }
});

test('GUI entry propagates exit receipt persistence failure without changing launch acceptance', async () => {
  const f = fixture();
  const launched = Promise.withResolvers();
  const error = new Error('exit receipt persistence failed');
  const pending = launchNativeGuiAndRecordExit(f.spec, {
    launch: async spec => {
      const result = await launchNativeGui(spec, f.deps);
      launched.resolve(result.receipt);
      return result;
    },
    persist: async () => { throw error; },
  });
  const rejected = assert.rejects(pending, actual => actual === error);
  const receipt = await launched.promise;
  f.child.emit('exit', 9, null);
  await rejected;
  assert.equal(receipt.status, 'spawned');
  assert.deepEqual(f.calls, ['spawn', 'persist']);
  assert.deepEqual(f.killed, []);
});

test('records actual child identity durably with exit subscription installed before delivery', async () => {
  const f = fixture();
  const result = await launchNativeGui(f.spec, f.deps);
  assert.deepEqual(f.calls, ['spawn', 'persist']);
  assert.equal(result.receipt.processStart, 'start-71');
  assert.equal(result.receipt.operationId, 'op-1');
  f.child.emit('exit', 0, null);
  assert.deepEqual(await result.exit, { code: 0, signal: null });
  assert.deepEqual(f.killed, []);
});

test('rejects hash and isolated environment mismatch before spawn', async () => {
  for (const change of [(f) => { f.deps.hashFile = async () => 'other'; }, (f) => { f.spec.env.FERRYX_DATA_DIR = '/production'; }]) {
    const f = fixture(); change(f);
    await assert.rejects(launchNativeGui(f.spec, f.deps));
    assert.deepEqual(f.calls, []);
  }
});

test('failed durable receipt kills only spawn-recorded child after exact identity recheck', async () => {
  const f = fixture();
  f.deps.persist = async () => { throw new Error('disk rejected'); };
  await assert.rejects(launchNativeGui(f.spec, f.deps), (error) => {
    assert.deepEqual(error.cleanup, { pid: 71, signalled: true, exitConfirmed: false }); return true;
  });
  assert.deepEqual(f.killed, [{ pid: 71, signal: 'SIGTERM' }]);
});

test('unreadable identity and replaced process start are reported without kill', async () => {
  const unreadable = fixture();
  unreadable.deps.probe = async () => { throw new Error('identity denied'); };
  await assert.rejects(launchNativeGui(unreadable.spec, unreadable.deps));
  assert.deepEqual(unreadable.killed, []);
  const replaced = fixture();
  let probes = 0;
  replaced.deps.probe = async () => ({ ...replaced.identity, processStart: ++probes === 1 ? 'start-71' : 'replacement' });
  replaced.deps.persist = async () => { throw new Error('disk rejected'); };
  await assert.rejects(launchNativeGui(replaced.spec, replaced.deps));
  assert.equal(probes, 2);
  assert.deepEqual(replaced.killed, []);
});

test('early exit never delivers ready identity', async () => {
  const f = fixture();
  f.deps.probe = async () => { f.child.emit('exit', 1, null); return f.identity; };
  f.deps.persist = async (_path, receipt) => { assert.equal(receipt.status, 'failed'); };
  await assert.rejects(launchNativeGui(f.spec, f.deps));
  assert.deepEqual(f.calls, ['spawn']);
  assert.deepEqual(f.killed, []);
});

test('actual filesystem publication never overwrites destination or deletes foreign pending evidence', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'native-launcher-contract-'));
  try {
    const path = join(directory, 'gui-launch.json');
    await persistLaunch(path, { status: 'spawned', pid: 71 });
    const original = await readFile(path, 'utf8');
    await writeFile(`${path}.pending`, 'foreign evidence', { flag: 'wx' });
    await assert.rejects(persistLaunch(path, { status: 'failed' }), { code: 'EEXIST' });
    assert.equal(await readFile(path, 'utf8'), original);
    assert.equal(await readFile(`${path}.pending`, 'utf8'), 'foreign evidence');
    assert.deepEqual((await readdir(directory)).sort(), ['gui-launch.json', 'gui-launch.json.pending']);
  } finally { await rm(directory, { recursive: true }); }
});

test('early exit during durable publication preserves success and emits separate immutable failure', async () => {
  const f = fixture();
  const directory = await mkdtemp(join(tmpdir(), 'native-launcher-contract-'));
  const path = join(directory, 'gui-launch.json');
  try {
    f.deps.persist = async (destination, raw) => {
      await persistLaunch(destination.endsWith('.failure.json') ? `${path}.failure.json` : path, raw);
      if (raw.status === 'spawned') f.child.emit('exit', 1, null);
    };
    await assert.rejects(launchNativeGui(f.spec, f.deps));
    assert.equal(JSON.parse(await readFile(path, 'utf8')).status, 'spawned');
    assert.equal(JSON.parse(await readFile(`${path}.failure.json`, 'utf8')).status, 'failed');
    assert.deepEqual(f.killed, []);
  } finally { await rm(directory, { recursive: true }); }
});

test('false signal delivery never claims signalled or confirmed exit', async () => {
  const f = fixture();
  f.child.kill = () => false;
  f.deps.persist = async () => { throw new Error('disk rejected'); };
  await assert.rejects(launchNativeGui(f.spec, f.deps), (error) => {
    assert.deepEqual(error.cleanup, { pid: 71, signalled: false, exitConfirmed: false }); return true;
  });
});
