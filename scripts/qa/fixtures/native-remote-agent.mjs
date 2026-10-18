// Runs only when the sole verifier starts it over SSH. JSONL RPC, no GUI actions.
import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { watch } from 'node:fs';
import { mkdir, readFile, readdir, rm, stat, realpath, open, link, lstat, unlink, rename, writeFile, chmod } from 'node:fs/promises';
import { createConnection } from 'node:net';
import { StringDecoder } from 'node:string_decoder';
import { createInterface } from 'node:readline';
import { resolve, relative, join, isAbsolute } from 'node:path';
import { BarrierHub, ATTACH_TUPLE_FIELDS, correlateReceipt, requireSevenTupleReceipt } from '../../lib/qa-scenarios/common-harness.mjs';
import { ensureFrontendServed, FRONTEND_DEV_URL } from '../../lib/qa-scenarios/frontend-server.mjs';
import { buildWindowsOwnedWindowsScript, buildWindowsNewPaneScript, orderOwnedWindowsForSearch } from '../../lib/qa-scenarios/native-driver.mjs';
import { parseWindowsProbeLine, buildInteractiveRelaunchPlan } from '../../lib/qa-scenarios/windows-interactive.mjs';

const children = new Map();
const watches = new Map();
const jobs = new Map();
const profiles = new Set();
const frontendProfiles = new Map();
const barriers = new Map();
const forwarderTransports = new Map();
const fixtureEndpoints = new Map();
const fixtureReadyWatches = new Map();
const ownershipStreams = new Map();
const localOwnerRpcs = new Map();
const spawnOwned = new Map();
const setupGuiReceipts = new Map();
const pendingSetupTasks = new Map();
const activeProbeWorkers = new Map();
let identityProbeBinary;
const output = (value) => process.stdout.write(`${JSON.stringify(value)}\n`);
const fail = (message, code = 'NATIVE_SETUP_UNIMPLEMENTED') => Object.assign(new Error(message), { code });
const inside = (root, path) => {
  const delta = relative(root, path);
  return delta && delta !== '..' && !delta.startsWith(`..${process.platform === 'win32' ? '\\' : '/'}`) && !isAbsolute(delta);
};
const ps = (value) => `'${String(value).replaceAll("'", "''")}'`;

export function subscribeSetupProbe(target, signal, timeoutMs = 150000) {
  let dispose;
  const promise = new Promise((resolveProbe, reject) => {
    let chain = Promise.resolve();
    const finish = (error, result) => { dispose(); error ? reject(error) : resolveProbe(result); };
    const completionPath = typeof target === 'string' ? target : target?.completionPath;
    const markerPath = typeof target === 'string' ? join(resolve(target, '..'), 'wrapper-exit.marker') : target?.markerPath;
    let result = null;
    let wrapperExitCode = null;
    const inspect = async () => {
      try {
        if (!result && completionPath) {
          try { result = JSON.parse(await readFile(completionPath, 'utf8')); }
          catch (error) { if (error.code !== 'ENOENT') return finish(error); }
        }
        if (markerPath && wrapperExitCode === null) {
          try {
            const rawMarker = (await readFile(markerPath, 'utf8')).trim();
            if (rawMarker.length > 0) {
              const code = Number(rawMarker);
              if (Number.isInteger(code)) wrapperExitCode = code;
            }
          } catch (error) { if (error.code !== 'ENOENT') return finish(error); }
        }
        const hasResult = Boolean(result);
        const hasMarker = markerPath ? wrapperExitCode !== null : true;
        if (hasResult && hasMarker) {
          if (wrapperExitCode !== null && wrapperExitCode !== 0) {
            return finish(fail(`Interactive setup probe wrapper exited with code ${wrapperExitCode}; task retained, no process killed`));
          }
          return finish(null, { ...result, wrapperExitCode: wrapperExitCode ?? 0 });
        }
      } catch (error) {
        finish(error);
      }
    };
    const abort = () => finish(signal.reason);
    const enqueue = () => { chain = chain.then(inspect); };
    const watchDir = resolve(completionPath ?? markerPath, '..');
    const watcher = watch(watchDir, enqueue);
    const poll = setInterval(enqueue, 250);
    const timer = setTimeout(() => finish(fail('Interactive setup probe completion deadline; task retained, no process killed')), timeoutMs);
    dispose = () => { watcher.close(); clearInterval(poll); clearTimeout(timer); signal.removeEventListener('abort', abort); };
    signal.addEventListener('abort', abort, { once: true });
    if (signal.aborted) abort(); else enqueue();
  });
  promise.catch(() => {});
  return { promise, close: () => dispose() };
}

export async function runInteractiveSetupProbe({ profile, gui, script, signal }, {
  subscribe = subscribeSetupProbe, invoke = command, persist = persistLaunch, makeDirectory = mkdir,
  saveScript = writeFile, pendingTasks = pendingSetupTasks,
  saveReport = (path, value) => writeFile(path, `${JSON.stringify(value)}\n`, { mode: 0o600 }),
  token = randomUUID(), executable = process.execPath, runner = resolve(process.argv[1]),
} = {}) {
  signal.throwIfAborted();
  const directory = join(profile, 'evidence', 'setup-probes', token);
  await makeDirectory(directory, { recursive: true });
  const specPath = join(directory, 'probe.json');
  const completionPath = join(directory, 'completion.json');
  const ownerPath = join(directory, 'owner.json');
  const wrapperMarkerPath = join(directory, 'wrapper-exit.marker');
  await persist(specPath, { gui, script, completionPath, ownerPath });
  const plan = buildInteractiveRelaunchPlan({ taskName: `ferryx-setup-${token}`, batDir: directory,
    nodePath: executable, runnerPath: runner, runnerArgs: ['--read-only-setup-probe', specPath], cwd: profile });
  await saveScript(plan.batPath, plan.batBody, { encoding: 'ascii', mode: 0o600 });
  // Task Scheduler starts the GUI-subsystem script host, not cmd/Bun. WSH's
  // window style 0 hides the INITIAL console and its inherited Bun console.
  // The trailing marker is written and closed after shell.Run returns, proving
  // that cmd.exe and the runner have exited. It does not prove wscript.exe OS
  // process termination, which occurs immediately afterward on WScript.Quit.
  const hiddenPath = join(directory, 'hidden-probe.vbs');
  const hiddenCommand = `cmd.exe /d /s /c ""${plan.batPath}""`;
  await saveScript(hiddenPath, [
    'Option Explicit',
    'Dim shell, fso, exitCode, marker',
    'Set shell = CreateObject("WScript.Shell")',
    `exitCode = shell.Run("${hiddenCommand.replaceAll('"', '""')}", 0, True)`,
    'Set fso = CreateObject("Scripting.FileSystemObject")',
    `Set marker = fso.CreateTextFile("${wrapperMarkerPath.replaceAll('"', '""')}", True)`,
    'marker.WriteLine exitCode',
    'marker.Close',
    'WScript.Quit exitCode',
  ].join('\r\n'), { encoding: 'ascii', mode: 0o600 });
  const createArgs = [...plan.createArgs];
  createArgs[createArgs.indexOf('/tr') + 1] = `wscript.exe //B //Nologo "${hiddenPath}"`;
  const subscribed = subscribe({ completionPath, markerPath: wrapperMarkerPath }, signal);
  const report = { taskName: plan.taskName, ownerPath, completionPath, wrapperMarkerPath, completed: false, processKillAttempted: false };
  try {
    if (subscribe === subscribeSetupProbe && invoke === command && activeProbeWorkers.get(profile)) {
      const workerDir = join(profile, 'evidence', 'setup-probes', 'worker-queue');
      const fastSubscribed = subscribe({ completionPath, markerPath: null }, signal);
      await persist(join(workerDir, `${token}.req.json`), { id: token, gui, script, completionPath });
      const result = await fastSubscribed.promise;
      report.completed = true;
      report.result = result;
      await saveReport(join(profile, 'evidence', 'setup-probe-dispatch.json'), report);
      if (result.error) throw fail(`Interactive setup probe failed: ${result.error}`);
      const session = parseWindowsProbeLine(result.stdout, 'setup-probe-session');
      if (!session.ok || session.probe.targetPid !== gui.pid || session.probe.probeSessionId <= 0 ||
          session.probe.probeSessionId !== session.probe.targetSessionId)
        throw fail('Interactive setup probe did not execute in the target GUI session');
      return result.stdout;
    }
    await invoke('schtasks', createArgs, signal);
    let tasks = pendingTasks.get(profile);
    if (!tasks) {
      tasks = new Set();
      pendingTasks.set(profile, tasks);
    }
    tasks.add(plan.taskName);
    await saveReport(join(profile, 'evidence', 'setup-probe-dispatch.json'), report);
    await invoke('schtasks', plan.runArgs, signal);
    const fastFirst = subscribe === subscribeSetupProbe && invoke === command
      ? subscribe({ completionPath, markerPath: null }, signal)
      : null;
    const result = await (fastFirst ? fastFirst.promise : subscribed.promise);
    fastFirst?.close();
    if (subscribe === subscribeSetupProbe && invoke === command) {
      activeProbeWorkers.set(profile, { wrapperMarkerPath, subscribed, deleteArgs: plan.deleteArgs, taskName: plan.taskName });
    }
    report.completed = true;
    report.result = result;
    if (!activeProbeWorkers.has(profile)) {
      await invoke('schtasks', plan.deleteArgs, signal);
      tasks = pendingTasks.get(profile);
      if (tasks) {
        tasks.delete(plan.taskName);
        if (tasks.size === 0) pendingTasks.delete(profile);
      }
    }
    if (result.error) throw fail(`Interactive setup probe failed: ${result.error}`);
    const session = parseWindowsProbeLine(result.stdout, 'setup-probe-session');
    if (!session.ok || session.probe.targetPid !== gui.pid || session.probe.probeSessionId <= 0 ||
        session.probe.probeSessionId !== session.probe.targetSessionId)
      throw fail('Interactive setup probe did not execute in the target GUI session');
    return result.stdout;
  } catch (error) {
    report.error = error.message;
    throw error;
  } finally {
    subscribed.close();
    await saveReport(join(profile, 'evidence', 'setup-probe-dispatch.json'), report);
  }
}

export function buildSetupProbeSessionScript(gui) {
  return [
    "$ErrorActionPreference = 'Stop';",
    `$target = Get-Process -Id ${gui.pid} -ErrorAction Stop;`,
    `if ($target.Path -ne ${ps(gui.executable)} -or $target.StartTime.ToUniversalTime().Ticks.ToString() -ne ${ps(gui.processStart)}) { throw 'Setup target identity changed' };`,
    '$probeSession = (Get-Process -Id $PID).SessionId;',
    "@{ probe = 'setup-probe-session'; targetPid = $target.Id; probeSessionId = $probeSession; targetSessionId = $target.SessionId } | ConvertTo-Json -Compress;",
    "if ($probeSession -le 0 -or $probeSession -ne $target.SessionId) { throw 'Setup probe session differs from target GUI session' };",
  ].join('\n');
}

export async function inspectSetupTarget({ profile, gui, point, hwnd, signal }, {
  ownedProfiles = profiles, receipts = setupGuiReceipts, platform = process.platform, inspect = probe,
  load = async path => JSON.parse(await readFile(path, 'utf8')),
  run = script => runInteractiveSetupProbe({ profile, gui, script, signal }),
  save = (path, diagnostics) => writeFile(path, `${JSON.stringify(diagnostics)}\n`, { mode: 0o600, signal }),
} = {}) {
  if (!ownedProfiles.has(profile) || platform !== 'win32') throw fail('Setup target requires an owned Windows profile');
  const spec = await load(join(profile, 'gui-launch-spec.json'));
  let receipt = receipts.get(profile);
  if (!receipt) {
    receipt = await load(join(profile, 'receipts', 'gui-launch.json'));
    if (receipt.role !== 'gui' || receipt.status !== 'spawned' || receipt.executable !== spec.binary ||
        !Number.isSafeInteger(receipt.pid) || receipt.pid <= 0 || !receipt.processStart ||
        ['runId', 'operationId', 'candidate', 'hostId'].some(key => receipt[key] !== spec[key]))
      throw fail('Setup GUI launch receipt disagrees with provisioned launch');
    receipt = Object.freeze({ ...receipt });
    receipts.set(profile, receipt);
  }
  if (['pid', 'processStart', 'executable', 'runId', 'operationId', 'candidate', 'hostId'].some(key => gui?.[key] !== receipt[key]))
    throw fail('Setup GUI disagrees with immutable launch receipt');
  const check = async () => {
    signal.throwIfAborted();
    const live = await inspect(receipt.pid, signal);
    if (live.exited !== false || live.executable !== receipt.executable || live.processStart !== receipt.processStart)
      throw fail('Setup GUI exact live identity changed');
  };
  await check();
  if (point) {
    if (!Number.isSafeInteger(hwnd) || hwnd <= 0 || !Number.isFinite(point.x) || !Number.isFinite(point.y)) throw fail('Invalid setup hit-test target');
    const raw = await run([
      'Add-Type @"',
      'using System; using System.Runtime.InteropServices;',
      'public class FerryxSetupHit {',
      ' [StructLayout(LayoutKind.Sequential)] public struct Point { public int X; public int Y; }',
      ' [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();',
      ' [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point p);',
      ' [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);',
      ' [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);',
      '}', '"@;',
      `$p = New-Object FerryxSetupHit+Point; $p.X = ${Math.round(point.x)}; $p.Y = ${Math.round(point.y)};`,
      '$fg = [FerryxSetupHit]::GetForegroundWindow(); $owner = [uint32]0;',
      '[FerryxSetupHit]::GetWindowThreadProcessId($fg, [ref]$owner) | Out-Null;',
      '$hit = [FerryxSetupHit]::GetAncestor([FerryxSetupHit]::WindowFromPoint($p), 2);',
      "@{ probe = 'setup-hit'; foregroundPid = $owner; foregroundHwnd = $fg.ToInt64(); hitHwnd = $hit.ToInt64() } | ConvertTo-Json -Compress;",
    ].join('\n'));
    const parsed = parseWindowsProbeLine(raw, 'setup-hit');
    await check();
    if (!parsed.ok) throw fail('Setup foreground hit-test unreadable');
    return { ...parsed.probe, owned: parsed.probe.foregroundPid === receipt.pid &&
      parsed.probe.foregroundHwnd === hwnd && parsed.probe.hitHwnd === hwnd };
  }
  const enumerationRaw = await run(buildWindowsOwnedWindowsScript(receipt.pid));
  const enumeration = parseWindowsProbeLine(enumerationRaw, 'owned-windows');
  const diagnostics = { gui: receipt, enumerationRaw, enumeration };
  const diagnosticPath = join(profile, 'evidence', 'setup-target.json');
  if (!enumeration.ok || enumeration.probe.pid !== receipt.pid) {
    diagnostics.rejections = [!enumeration.ok ? 'enumeration-unreadable' : 'enumeration-pid-mismatch'];
    await save(diagnosticPath, diagnostics);
    throw fail('Setup owned-window enumeration failed');
  }
  const windows = orderOwnedWindowsForSearch(enumeration.probe.windows, enumeration.probe.mainWindowHandle);
  const newPaneRaw = await run(buildWindowsNewPaneScript(receipt.pid, { windows, readOnly: true }));
  const result = parseWindowsProbeLine(newPaneRaw, 'new-pane');
  await check();
  const target = result.probe?.chosen;
  const rejections = Object.entries({
    'new-pane-unreadable': !result.ok,
    'new-pane-pid-mismatch': result.probe?.pid !== receipt.pid,
    'new-pane-result-mismatch': result.probe?.result !== 'PANE_TARGET',
    'actionable-count-not-one': result.probe?.actionableCount !== 1,
    'chosen-window-not-owned': !windows.some(window => window.hwnd === target?.windowHwnd),
    'chosen-name-mismatch': target?.name !== 'New Terminal',
    'chosen-not-enabled': target?.enabled !== true,
    'chosen-not-onscreen': target?.offscreen !== false,
    'chosen-rectangle-empty': target?.rectEmpty !== false,
  }).filter(([, rejected]) => rejected).map(([condition]) => condition);
  await save(diagnosticPath, { ...diagnostics, windows, newPaneRaw, newPane: result, rejections });
  if (rejections.length) throw fail('Setup New Terminal target refused');
  const rect = String(target.rect).split(',').map(Number);
  if (rect.length !== 4 || !rect.every(Number.isFinite) || rect[2] <= 0 || rect[3] <= 0) throw fail('Invalid setup target rectangle');
  return { gui: receipt, hwnd: target.windowHwnd, point: { x: Math.round(rect[0] + rect[2] / 2), y: Math.round(rect[1] + rect[3] / 2) } };
}

export function buildWindowsChildWindowsScript(pid) {
  return [
    "$ErrorActionPreference = 'Stop';",
    'Add-Type @"',
    'using System;',
    'using System.Text;',
    'using System.Runtime.InteropServices;',
    'public class FerryxQaChildren {',
    '  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }',
    '  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);',
    '  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);',
    '  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr hWndParent, EnumProc cb, IntPtr lParam);',
    '  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);',
    '  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);',
    '  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);',
    '  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr hWnd, StringBuilder text, int count);',
    '}',
    '"@;',
    `$targetPid = ${pid};`,
    '$proc = Get-Process -Id $targetPid -ErrorAction SilentlyContinue;',
    'if ($proc -eq $null) {',
    "  Write-Output (@{ probe = 'setup-child-windows'; pid = $targetPid; processExited = $true; visibleTopHwnds = @(); children = @() } | ConvertTo-Json -Compress);",
    '  exit 0;',
    '}',
    '$topWindows = New-Object System.Collections.ArrayList;',
    '$topCb = [FerryxQaChildren+EnumProc]{ param($hWnd, $lParam)',
    '  $topOwner = [uint32]0;',
    '  [FerryxQaChildren]::GetWindowThreadProcessId($hWnd, [ref]$topOwner) | Out-Null;',
    '  if ($topOwner -eq [uint32]$targetPid -and [bool][FerryxQaChildren]::IsWindowVisible($hWnd)) {',
    '    $topWindows.Add($hWnd) | Out-Null;',
    '  }',
    '  return $true;',
    '};',
    '[FerryxQaChildren]::EnumWindows($topCb, [IntPtr]::Zero) | Out-Null;',
    'if ($topWindows.Count -eq 0) {',
    "  Write-Output (@{ probe = 'setup-child-windows'; pid = $targetPid; visibleTopHwnds = @(); children = @() } | ConvertTo-Json -Compress);",
    '  exit 0;',
    '}',
    '$children = New-Object System.Collections.ArrayList;',
    'foreach ($topHwnd in $topWindows) {',
    '  $currentTop = $topHwnd.ToInt64();',
    '  $childCb = [FerryxQaChildren+EnumProc]{ param($hChild, $lParam)',
    '    $childOwner = [uint32]0;',
    '    [FerryxQaChildren]::GetWindowThreadProcessId($hChild, [ref]$childOwner) | Out-Null;',
    '    $r = New-Object FerryxQaChildren+RECT;',
    '    [FerryxQaChildren]::GetWindowRect($hChild, [ref]$r) | Out-Null;',
    '    $cls = New-Object System.Text.StringBuilder 256;',
    '    [FerryxQaChildren]::GetClassNameW($hChild, $cls, 256) | Out-Null;',
    '    $w = $r.Right - $r.Left;',
    '    $h = $r.Bottom - $r.Top;',
    '    $children.Add([ordered]@{',
    '      hwnd = $hChild.ToInt64();',
    '      topHwnd = $currentTop;',
    '      ownerPid = [int]$childOwner;',
    '      className = $cls.ToString();',
    '      visible = [bool][FerryxQaChildren]::IsWindowVisible($hChild);',
    '      rect = [ordered]@{ left = $r.Left; top = $r.Top; right = $r.Right; bottom = $r.Bottom; width = $w; height = $h };',
    '    }) | Out-Null;',
    '    return $true;',
    '  };',
    '  [FerryxQaChildren]::EnumChildWindows($topHwnd, $childCb, [IntPtr]::Zero) | Out-Null;',
    '}',
    '$topHwnds = @($topWindows | ForEach-Object { $_.ToInt64() });',
    '$payload = [ordered]@{',
    "  probe = 'setup-child-windows';",
    '  pid = $targetPid;',
    '  visibleTopHwnds = $topHwnds;',
    '  children = $children;',
    '};',
    'Write-Output ($payload | ConvertTo-Json -Compress -Depth 6);',
  ].join('\n');
}

export async function inspectSetupChildWindows({ profile, timeoutMs = 5000, signal }, {
  ownedProfiles = profiles, receipts = setupGuiReceipts, platform = process.platform, inspect = probe,
  load = async path => JSON.parse(await readFile(path, 'utf8')),
  run = (script, targetGui, probeSignal) => runInteractiveSetupProbe({ profile, gui: targetGui, script, signal: probeSignal }),
  save = (path, diagnostics, probeSignal) => writeFile(path, `${JSON.stringify(diagnostics)}\n`, { mode: 0o600, signal: probeSignal }),
} = {}) {
  if (!ownedProfiles.has(profile) || platform !== 'win32') throw fail('Setup child diagnostic requires an owned Windows profile');
  const spec = await load(join(profile, 'gui-launch-spec.json'));
  let receipt = receipts.get(profile);
  if (!receipt) {
    receipt = await load(join(profile, 'receipts', 'gui-launch.json'));
    if (receipt.role !== 'gui' || receipt.status !== 'spawned' || receipt.executable !== spec.binary ||
        !Number.isSafeInteger(receipt.pid) || receipt.pid <= 0 || !receipt.processStart ||
        ['runId', 'operationId', 'candidate', 'hostId'].some(key => receipt[key] !== spec[key]))
      throw fail('Setup GUI launch receipt disagrees with provisioned launch');
    receipt = Object.freeze({ ...receipt });
    receipts.set(profile, receipt);
  }
  signal?.throwIfAborted();
  const probeController = new AbortController();
  const abortProbe = () => probeController.abort(signal?.reason);
  if (signal?.aborted) probeController.abort(signal.reason);
  else signal?.addEventListener('abort', abortProbe, { once: true });
  const timer = setTimeout(() => probeController.abort(fail('Setup child diagnostic timeout')), timeoutMs);
  const probeSignal = probeController.signal;
  try {
    const live = await inspect(receipt.pid, probeSignal);
    if (live.exited !== false || live.executable !== receipt.executable || live.processStart !== receipt.processStart)
      throw fail('Setup GUI exact live identity changed');
    const raw = await run(buildWindowsChildWindowsScript(receipt.pid), receipt, probeSignal);
    const parsed = parseWindowsProbeLine(raw, 'setup-child-windows');
    if (!parsed.ok || parsed.probe?.pid !== receipt.pid) {
      throw fail('Setup child window enumeration unreadable or PID mismatch');
    }
    if (!Array.isArray(parsed.probe?.visibleTopHwnds) ||
        !parsed.probe.visibleTopHwnds.every(h => Number.isSafeInteger(h) && h > 0)) {
      throw fail('Setup child window visibleTopHwnds malformed');
    }
    const visibleTopHwnds = parsed.probe.visibleTopHwnds;
    const visibleTopSet = new Set(visibleTopHwnds);
    if (!Array.isArray(parsed.probe?.children)) {
      throw fail('Setup child window enumeration children malformed');
    }
    const children = parsed.probe.children;
    for (const child of children) {
      if (!Number.isSafeInteger(child.hwnd) || child.hwnd <= 0 ||
          !Number.isSafeInteger(child.topHwnd) || !visibleTopSet.has(child.topHwnd) ||
          !Number.isSafeInteger(child.ownerPid) || child.ownerPid <= 0 ||
          typeof child.className !== 'string' ||
          typeof child.visible !== 'boolean' ||
          !child.rect || typeof child.rect !== 'object' ||
          !['left', 'top', 'right', 'bottom', 'width', 'height'].every(k => Number.isFinite(child.rect[k]))) {
        throw fail('Setup child window entry malformed');
      }
    }
    const report = {
      gui: receipt,
      visibleTopHwnds,
      childCount: children.length,
      children,
      raw,
    };
    const reportPath = join(profile, 'evidence', 'setup-child-windows.json');
    await save(reportPath, report, probeSignal);
    return report;
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', abortProbe);
  }
}

export function createRemoteFrontend({ serve = ensureFrontendServed } = {}) {
  const servers = new Set();
  let closed = false;
  let closing;
  const closeServer = (server) => new Promise((resolveClose, reject) => {
    server.close(error => error ? reject(error) : resolveClose());
    server.closeAllConnections?.();
  });
  const close = () => {
    closed = true;
    closing ??= Promise.all([...servers].map(closeServer));
    return closing;
  };
  return {
    close,
    async start({ worktree, uiDist, frontend, signal }) {
      if (frontend?.mode !== 'debug' || frontend.url !== FRONTEND_DEV_URL) {
        throw fail('Explicit frontend mode debug and matching dev URL are required');
      }
      signal.throwIfAborted();
      if (closed || servers.size) throw fail('Frontend resource already started or closed');
      const abort = () => { close().catch(() => {}); };
      signal.addEventListener('abort', abort, { once: true });
      try {
        const result = await serve({ rootDir: worktree, distDir: uiDist,
          registry: { registerServer(server) {
            if (closed) {
              closing = Promise.all([closing, closeServer(server)]);
              return;
            }
            servers.add(server);
          } } });
        signal.throwIfAborted();
        if (closed) throw fail('Frontend resource closed during setup');
        const { server, ...receipt } = result;
        return receipt;
      } catch (error) {
        await close();
        throw error;
      } finally {
        signal.removeEventListener('abort', abort);
      }
    },
  };
}

export async function persistLaunch(path, receipt) {
  const temporary = `${path}.${randomUUID()}.pending`;
  const file = await open(temporary, 'wx');
  const owned = await file.stat();
  try {
    await file.writeFile(`${JSON.stringify(receipt)}\n`);
    await file.sync();
    // Hard-link publication is atomic and fails EEXIST rather than replacing
    // an earlier receipt. Only this invocation's temporary inode is cleaned up.
    await link(temporary, path);
  } finally {
    await file.close();
    const current = await lstat(temporary);
    if (current.dev === owned.dev && current.ino === owned.ino) await unlink(temporary);
  }
}

export async function launchNativeDaemon(spec, deps = {}) {
  const io = { spawn, probe: (pid) => probe(pid, spec.signal, spec.setup.binary), persist: persistLaunch,
    realpath, hashFile, ...deps };
  const { setup, runId, operationId, hostId, timeoutMs, signal } = spec;
  const binary = setup.binary;
  if (!runId || !operationId || !hostId || !setup.candidate || !Number.isFinite(timeoutMs) || timeoutMs <= 0 ||
      resolve(setup.profile) !== join(resolve(setup.worktree), '.native-qa', runId) || !inside(setup.worktree, binary)) {
    throw fail('Invalid isolated daemon specification');
  }
  const required = { FERRYX_QA_ISOLATED: '1', FERRYX_DATA_DIR: join(setup.profile, 'data'),
    FERRYX_RUNTIME_DIR: join(setup.profile, 'runtime'), FERRYX_SESSION_DIR: join(setup.profile, 'session'),
    FERRYX_QA_BARRIER_DIR: join(setup.profile, 'barriers'), FERRYX_QA_RUN_ID: runId,
    FERRYX_QA_OPERATION_ID: operationId, HOME: join(setup.profile, 'home'), USERPROFILE: join(setup.profile, 'home') };
  if (Object.entries(required).some(([key, value]) => setup.env?.[key] !== value)) throw fail('Daemon environment is not isolated');
  if (await io.realpath(binary) !== binary || await io.hashFile(binary) !== setup.binarySha256) throw fail('Daemon candidate hash/path mismatch');
  signal.throwIfAborted();
  const child = io.spawn(binary, ['--daemon'], { cwd: setup.worktree, env: { ...process.env, ...setup.env }, stdio: ['ignore', 'pipe', 'pipe'] });
  const pid = child.pid;
  let ended = false;
  let rejectReady;
  const exit = new Promise((resolveExit) => {
    child.once('exit', (code, exitSignal) => { ended = true; rejectReady?.(fail('Daemon exited before receipt')); resolveExit({ code, signal: exitSignal }); });
    child.once('error', (error) => { ended = true; rejectReady?.(error); resolveExit({ error: error.message }); });
  });
  if (!Number.isSafeInteger(pid) || pid <= 0) throw fail('Daemon spawn has no exact PID');
  children.set(pid, { child, exit });
  let pending = '';
  let receive;
  let abort;
  let timer;
  const ready = new Promise((resolveReady, reject) => {
    rejectReady = reject;
    receive = (chunk) => {
      pending += chunk.toString();
      let newline;
      while ((newline = pending.indexOf('\n')) >= 0) {
        const line = pending.slice(0, newline).replace(/\r$/, ''); pending = pending.slice(newline + 1);
        if (line === 'FERRYX_DAEMON_READY') resolveReady();
      }
    };
    abort = () => reject(signal.reason);
    child.stdout.on('data', receive);
    child.stderr.resume();
    signal.addEventListener('abort', abort, { once: true });
    timer = setTimeout(() => reject(fail('Daemon readiness deadline')), timeoutMs);
    if (signal.aborted) abort();
  });
  ready.catch(() => {});
  try {
    const initial = await io.probe(pid);
    if (ended || initial.exited !== false || initial.executable !== binary || !initial.processStart) throw fail('Daemon initial identity refused');
    await ready;
    signal.throwIfAborted();
    const final = await io.probe(pid);
    if (ended || final.exited !== false || final.executable !== initial.executable || final.processStart !== initial.processStart) throw fail('Daemon ready identity replaced or exited');
    signal.throwIfAborted();
      const receipt = { runId, operationId, hostId, candidate: setup.candidate, role: 'daemon', status: 'ready',
      pid, executable: final.executable, processStart: final.processStart, readiness: 'FERRYX_DAEMON_READY' };
    if (process.platform === 'win32') {
      // Windows serves the daemon on loopback TCP and publishes the port beside daemon.token;
      // the forwarder dials it, so the receipt carries the port the daemon itself wrote.
      const port = Number((await readFile(join(setup.profile, 'runtime', 'daemon.port'), 'utf8')).trim());
      if (!Number.isSafeInteger(port) || port <= 0 || port > 65535) throw fail('Daemon published no valid loopback port');
      receipt.port = port;
    }
    await io.persist(join(setup.profile, 'receipts', 'daemon-launch.json'), receipt);
      spawnOwned.set(setup.profile, new Map([[`${receipt.role}:${runId}:${pid}`, receipt]]));
    signal.throwIfAborted();
    if (ended) throw fail('Daemon exited during receipt publication');
    return receipt;
  } finally {
    clearTimeout(timer); child.stdout.removeListener('data', receive); signal.removeEventListener('abort', abort);
    // Exact child remains registered for reporting/explicit identity-checked teardown.
    // A refused identity never causes uncertain automatic cleanup.
  }
}

export async function launchNativeGui(spec, deps = {}) {
  const io = { spawn, open, probe: (pid) => probe(pid, undefined, spec.binary), persist: persistLaunch, realpath, hashFile,
    validateWorktree: async (path) => {
      const root = await command('git', ['-C', path, 'rev-parse', '--show-toplevel']);
      const linked = await command('git', ['-C', path, 'rev-parse', '--git-dir']);
      if (resolve(root) !== resolve(path) || linked === '.git') throw fail('GUI launcher requires selected linked worktree');
    }, ...deps };
  const { worktreeRoot, worktree, profile, binary, binarySha256, runId, operationId, candidate, hostId, env, receiptPath } = spec;
  if (!runId || !/^[a-z0-9-]+$/.test(runId) || !operationId || !candidate || !hostId ||
      !inside(worktreeRoot, worktree) || resolve(profile) !== join(resolve(worktree), '.native-qa', runId) ||
      !inside(worktree, binary) || resolve(receiptPath) !== join(resolve(profile), 'receipts', 'gui-launch.json')) {
    throw fail('Invalid isolated GUI launch specification');
  }
  for (const path of [worktreeRoot, worktree, profile, binary]) {
    if (resolve(await io.realpath(path)) !== resolve(path)) throw fail('GUI launch path is not canonical');
  }
  await io.validateWorktree(worktree);
  if (await io.hashFile(binary) !== binarySha256) throw fail('GUI candidate hash mismatch');
  const required = { FERRYX_QA_ISOLATED: '1', FERRYX_DATA_DIR: join(profile, 'data'),
    FERRYX_RUNTIME_DIR: join(profile, 'runtime'), FERRYX_SESSION_DIR: join(profile, 'session'),
    FERRYX_QA_BARRIER_DIR: join(profile, 'barriers'), FERRYX_QA_RUN_ID: runId,
    FERRYX_QA_OPERATION_ID: operationId, HOME: join(profile, 'home'), USERPROFILE: join(profile, 'home') };
  if (Object.entries(required).some(([key, value]) => env?.[key] !== value)) throw fail('GUI launch environment is not isolated');
  const handles = [];
  try {
  for (const name of ['gui-stdout.log', 'gui-stderr.log']) {
    handles.push(await io.open(join(profile, 'evidence', name), 'wx', 0o600));
  }
  const child = io.spawn(binary, [], { cwd: worktree, env: { ...process.env, ...env },
    stdio: ['ignore', handles[0].fd, handles[1].fd] });
  const pid = child.pid;
  let ended = false;
  const exit = new Promise((resolveExit) => {
    child.once('exit', (code, signal) => { ended = true; resolveExit({ code, signal }); });
    child.once('error', (error) => { ended = true; resolveExit({ error: error.message }); });
  });
  let identity;
  try {
    if (!Number.isSafeInteger(pid) || pid <= 0) throw fail('GUI spawn has no exact child PID');
    identity = await io.probe(pid);
    if (ended || identity.exited || !identity.processStart || resolve(identity.executable ?? '') !== resolve(binary)) {
      throw fail('GUI spawn identity cannot be confirmed');
    }
    const receipt = { runId, operationId, candidate, hostId, role: 'gui', status: 'spawned', pid,
      executable: identity.executable, processStart: identity.processStart };
    await io.persist(receiptPath, receipt);
    const roster = spawnOwned.get(profile) ?? new Map();
    roster.set(`${receipt.role}:${runId}:${pid}`, receipt);
    spawnOwned.set(profile, roster);
    if (ended) throw fail('GUI exited before launch receipt delivery');
    return { receipt, child, exit };
  } catch (error) {
    // PID came from this spawn. Unreadable or replaced identity is reported,
    // never killed; no process-name search or retry supplies ownership.
    let live;
    try { if (identity?.processStart && !ended) live = await io.probe(pid); } catch { /* report below */ }
    if (live && !live.exited && live.processStart === identity.processStart &&
        resolve(live.executable ?? '') === resolve(binary) && resolve(identity.executable ?? '') === resolve(binary)) {
      try {
        error.cleanup = { pid, signalled: child.kill('SIGTERM') === true, exitConfirmed: false };
      } catch (signalError) {
        error.cleanup = { pid, signalled: false, exitConfirmed: false, error: signalError.message };
      }
    } else error.cleanup = { pid, signalled: false, reason: 'identity_unconfirmed_or_exited' };
    try { await io.persist(`${receiptPath}.failure.json`, { runId, operationId, candidate, hostId, role: 'gui', status: 'failed',
      pid, error: error.message, cleanup: error.cleanup }); }
    catch (persistError) { error.receiptError = persistError.message; }
    throw error;
  }
  } finally {
    await Promise.all(handles.map(handle => handle.close()));
  }
}

export async function launchNativeGuiAndRecordExit(spec, { launch = launchNativeGui, persist = persistLaunch } = {}) {
  const { receipt, exit } = await launch(spec);
  const outcome = await exit;
  const exitReceipt = { ...receipt, status: 'exited', ...outcome };
  await persist(`${spec.receiptPath}.exit.json`, exitReceipt);
  return exitReceipt;
}

export async function collectLaunchEvidence(profile, signal, { ownedProfiles = profiles, read = readFile } = {}) {
  if (!ownedProfiles.has(profile)) throw fail('Unowned launch evidence collection refused');
  const files = [];
  for (const name of ['receipts/gui-launch.json', 'receipts/gui-launch.json.exit.json',
    'evidence/launcher-entry.log', 'evidence/launcher-output.log', 'evidence/launcher-exit.log',
    'evidence/gui-stdout.log', 'evidence/gui-stderr.log', 'runtime/boot-trace.log', 'evidence/setup-target.json',
    'evidence/setup-probe-dispatch.json', 'evidence/setup-child-windows.json', 'tmp/ferryx-switch-debug.jsonl']) {
    signal.throwIfAborted();
    try {
      const bytes = await read(join(profile, name), { signal });
      files.push({ name, base64: bytes.toString('base64') });
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
      files.push({ name, missing: true });
    }
  }
  signal.throwIfAborted();
  return files;
}

function command(executable, args, signal) {
  return new Promise((resolveCommand, reject) => {
    const child = spawn(executable, args, { stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true, signal });
    let stdout = ''; let stderr = '';
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.once('error', reject);
    child.once('close', (code) => code === 0 ? resolveCommand(stdout.trim()) : reject(fail(`${executable}: ${code}: ${stderr}`, 'NATIVE_REMOTE_COMMAND_FAILED')));
  });
}

export async function waitWindowsExactPidExit(pid, processStart, signal, invoke = command) {
  signal.throwIfAborted();
  await invoke('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
    `try {$p=Get-Process -Id ${pid} -ErrorAction Stop} catch {if($_.FullyQualifiedErrorId -eq 'NoProcessFoundForGivenId,Microsoft.PowerShell.Commands.GetProcessCommand' -and $_.CategoryInfo.Category -eq [System.Management.Automation.ErrorCategory]::ObjectNotFound){exit 0}; throw}; if($p.StartTime.ToUniversalTime().Ticks.ToString() -ne ${ps(processStart)}){throw 'PID incarnation mismatch'}; $p.WaitForExit()`], signal);
  signal.throwIfAborted();
}

export async function probeDarwin(pid, binary, signal, invoke = command) {
  if (!Number.isSafeInteger(pid) || pid <= 0 || pid > 2147483647 || !isAbsolute(binary ?? '')) {
    throw fail('Darwin exact PID probe requires the isolated candidate binary');
  }
  const value = JSON.parse(await invoke(binary, ['qa-exact-pid-probe', String(pid)], signal));
  if (value.exited !== false || !isAbsolute(value.executable ?? '') ||
      !/^[0-9]+:[0-9]{6}$/.test(value.processStart ?? '')) {
    throw fail('Malformed Darwin libproc identity');
  }
  return value;
}

async function probe(pid, signal, binary = identityProbeBinary) {
  if (!Number.isSafeInteger(pid) || pid <= 0) throw fail('Invalid exact PID');
  if (process.platform === 'darwin') {
    const val = await probeDarwin(pid, binary, signal);
    try {
      const ppidRaw = await command('ps', ['-o', 'ppid=', '-p', String(pid)], signal);
      val.ppid = Number(ppidRaw.trim());
    } catch (_) {}
    return val;
  }
  if (process.platform === 'win32') {
    const raw = await command('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      `$p=Get-Process -Id ${pid} -ErrorAction SilentlyContinue; if($null -eq $p){'{"exited":true}'}else{$w=Get-CimInstance Win32_Process -Filter "ProcessId = ${pid}"; @{exited=$false;executable=$p.Path;processStart=$p.StartTime.ToUniversalTime().Ticks.ToString();ppid=$w.ParentProcessId}|ConvertTo-Json -Compress}`], signal);
    return JSON.parse(raw);
  }
  try {
    const raw = await readFile(`/proc/${pid}/stat`, 'utf8');
    const fields = raw.slice(raw.lastIndexOf(')') + 2).split(' ');
    const executable = await command('readlink', ['-f', `/proc/${pid}/exe`], signal);
    return { exited: false, executable, processStart: fields[19], ppid: Number(fields[1]) };
  } catch (error) {
    if (error.code === 'ENOENT') return { exited: true };
    throw error; // Permission/unreadable is never an exit receipt.
  }
}

async function hashFile(path) {
  return createHash('sha256').update(await readFile(path)).digest('hex');
}

async function hashTree(path) {
  const hash = createHash('sha256');
  async function visit(directory) {
    for (const name of (await readdir(directory)).sort()) {
      const file = join(directory, name);
      const info = await stat(file);
      if (info.isDirectory()) await visit(file);
      else { hash.update(relative(path, file).split('\\').join('/')); hash.update('\0'); hash.update(await readFile(file)); hash.update('\0'); }
    }
  }
  await visit(path);
  return hash.digest('hex');
}

// Correlated native snapshot request (see case 'snapshot').
export async function requestCorrelatedSnapshot(args, { watchDir = watch, write = writeFile, move = rename, read = readFile } = {}) {
  if (typeof args.runId !== 'string' || typeof args.operationId !== 'string' || !args.attachTuple ||
      !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(String(args.requestId))) {
    throw fail('Snapshot request requires runId, operationId, UUID requestId and attachTuple');
  }
  const timeoutMs = Math.min(Math.max(1, Number(args.timeoutMs) || 5_000), 5_000);
  const barrierDir = join(args.profile, 'barriers');
  const base = `native-snapshot-${args.requestId}`;
  const responsePath = join(barrierDir, `${base}.response.json`);
  return new Promise((resolveSnapshot, rejectSnapshot) => {
    let settled = false;
    let timer;
    let watcher;
    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      watcher?.close();
      if (error) rejectSnapshot(error); else resolveSnapshot(value);
    };
    const inspect = async () => {
      let text;
      try { text = await read(responsePath, 'utf8'); }
      catch (error) { if (error.code !== 'ENOENT') finish(error); return; }
      try { finish(null, JSON.parse(text)); } catch { /* partial write; atomic rename settles next event */ }
    };
    try {
      watcher = watchDir(barrierDir, (_, name) => { if (String(name) === `${base}.response.json`) void inspect(); });
    } catch (error) { finish(error); return; }
    // The product settles its own typed timeout inside timeoutMs; this
    // bound only guards a GUI that never answers at all.
    timer = setTimeout(() => finish(fail('Native snapshot response not settled by product within deadline', 'NATIVE_SNAPSHOT_TIMEOUT')), timeoutMs + 2_000);
    const request = { runId: args.runId, operationId: args.operationId, requestId: args.requestId,
      attachTuple: args.attachTuple, timeoutMs };
    const tmp = join(barrierDir, `.${base}.request.json.tmp-${process.pid}`);
    write(tmp, JSON.stringify(request), { flag: 'wx' })
      .then(() => move(tmp, join(barrierDir, `${base}.request.json`)))
      .then(() => inspect())
      .catch((error) => finish(error));
  });
}

export function createOwnedPtyStream({ path, read = readFile, subscribe = watch, emit, onError = () => {} }) {
  let chain = Promise.resolve();
  let previous = Buffer.alloc(0);
  let cursor = -1;
  let failure;
  let closed = false;
  const consume = async () => {
    let bytes;
    try { bytes = await read(path); } catch (error) { if (error.code === 'ENOENT' && previous.length === 0) return; throw error; }
    bytes = Buffer.from(bytes);
    if (bytes.length < previous.length || !bytes.subarray(0, previous.length).equals(previous)) {
      throw fail('Owned PTY receipt producer truncated or rewrote evidence');
    }
    const lines = bytes.toString('utf8').split('\n');
    lines.pop();
    for (let index = cursor + 1; index < lines.length; index++) {
      if (lines[index].trim()) emit({ path, lineIndex: index, raw: JSON.parse(lines[index]) });
      cursor = index;
    }
    previous = bytes;
  };
  const enqueue = () => {
    chain = chain.then(() => { if (failure) throw failure; if (!closed) return consume(); })
      .catch((error) => { if (!failure) onError(error); failure = error; });
    return chain;
  };
  const watcher = subscribe(resolve(path, '..'), (_, name) => {
    if (String(name) === 'owned-pty-creation.receipt.jsonl') void enqueue();
  });
  return {
    async cursor() { await enqueue(); if (failure) throw failure; return { path, afterLine: cursor }; },
    async drain() { await enqueue(); if (failure) throw failure; },
    close() { closed = true; watcher.close(); },
  };
}

export function createLocalPtyOwnerRpc({ profile, runId, operationId, read = readFile, write = writeFile,
  renameFile = rename, subscribe = watch, isProfileActive = (path) => profiles.has(path),
  ownerCommandId = randomUUID, timeoutMs = 10_000 }) {
  const directory = join(profile, 'barriers');
  const names = {
    identity: 'local-pty-owner-identity',
    open: 'local-pty-owner-open',
    read: 'local-pty-owner-read',
    close: 'local-pty-owner-close',
  };
  const instanceId = randomUUID();
  let pending = Promise.resolve();
  let closed = false;
  let commandSequence = 0;
  let lastCommandId;
  let auditId;
  let owner;
  let openCompleted = false;
  const enqueue = (operation) => { pending = pending.then(operation); return pending; };
  const awaitPhysicalFile = (file, signal) => new Promise((resolveWait, reject) => {
    let timer;
    let settled = false;
    const done = (error, value) => {
      if (settled) return;
      settled = true;
      watcher.close(); clearTimeout(timer); signal?.removeEventListener('abort', abort);
      error ? reject(error) : resolveWait(value);
    };
    const abort = () => done(signal.reason);
    const inspect = async () => {
      try { done(null, JSON.parse(await read(join(directory, file), 'utf8'))); }
      catch (error) { if (error.code !== 'ENOENT') done(error); }
    };
    const watcher = subscribe(directory, (_event, filename) => { if (String(filename) === file) void inspect(); });
    signal?.addEventListener('abort', abort, { once: true });
    timer = setTimeout(() => done(fail(`Local PTY owner ${file} deadline`, 'NATIVE_AUDIT_INCOMPLETE')), timeoutMs);
    if (closed || signal?.aborted) abort(); else void inspect();
  });
  async function request(command, args, signal) {
    if (closed || !isProfileActive(profile)) throw fail('Local PTY owner RPC profile is not active', 'NATIVE_AUDIT_INCOMPLETE');
    if (args.runId !== runId || args.operationId !== operationId ||
        (command !== 'identity' && (typeof args.auditId !== 'string' || !args.auditId ||
        (command === 'open' ? Boolean(auditId) : !openCompleted || args.auditId !== auditId)))) {
      throw fail('Local PTY owner command identity mismatch', 'NATIVE_AUDIT_INCOMPLETE');
    }
    if (command === 'open') auditId = args.auditId;
    const id = ownerCommandId();
    lastCommandId = id;
    // Identity discovery does not consume the producer audit's sequence 1 open.
    const commandSeq = command === 'identity' ? 0 : ++commandSequence;
    const requestName = `${names[command]}.request.json`;
    const commandFile = join(directory, requestName);
    const ackName = `${names[command]}.ack.${id}.json`;
    try { await read(commandFile); throw fail(`Local PTY owner ${command} command already pending`, 'NATIVE_AUDIT_INCOMPLETE'); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
    const payload = { ...args, commandId: id, commandSeq };
    const temporary = `${commandFile}.${id}.pending`;
    // Arm all physical receipt observers before publishing the command. The
    // producer may process it immediately and atomically write both receipts.
    const ackPromise = awaitPhysicalFile(ackName, signal);
    await write(temporary, `${JSON.stringify(payload)}\n`, { flag: 'wx' });
    await renameFile(temporary, commandFile);
    const ack = await ackPromise;
    if (ack.commandId !== id || ack.runId !== runId ||
        ack.operationId !== operationId || ack.accepted !== true) {
      throw fail(`Local PTY owner ${command} acknowledgment identity or acceptance mismatch`, 'NATIVE_AUDIT_INCOMPLETE');
    }
    if (command === 'identity') {
      const discovered = ack.owner;
      if (ack.commandSeq !== 0 || !discovered || typeof discovered.ownerId !== 'string' || !discovered.ownerId ||
          !Number.isSafeInteger(discovered.pid) || discovered.pid <= 0 ||
          typeof discovered.executable !== 'string' || !discovered.executable ||
          typeof discovered.processStart !== 'string' || !discovered.processStart || discovered.component !== 'terminal-local-pty')
        throw fail('Malformed local PTY owner identity acknowledgment', 'NATIVE_AUDIT_INCOMPLETE');
      if (owner && JSON.stringify(owner) !== JSON.stringify(discovered))
        throw fail('Local PTY owner identity changed before audit', 'NATIVE_AUDIT_INCOMPLETE');
      owner = discovered;
      return { ack, owner: { ...owner }, instanceId, commandSeq };
    }
    if (command === 'open') { openCompleted = true; return { ack, owner: null, instanceId, commandSeq }; }
    const packetName = command === 'read' ? 'local-pty-owner-read.receipt.json' : 'local-pty-owner-audit.json';
    if (ack.packetFile !== packetName) {
      throw fail(`Local PTY owner ${command} acknowledgment names the wrong packet`, 'NATIVE_AUDIT_INCOMPLETE');
    }
    // The producer writes/replaces the packet before atomically writing the
    // per-command ack. Read only after that exact ack, never accept a stale
    // fixed-name packet observed before command completion.
    const receipt = JSON.parse(await read(join(directory, packetName), 'utf8'));
    if (receipt.auditId !== args.auditId || receipt.runId !== runId || receipt.operationId !== operationId ||
        !receipt.owner || !Number.isSafeInteger(receipt.baselineSeq) || !Number.isSafeInteger(receipt.endSeq) ||
        !Array.isArray(receipt.requests) || !Array.isArray(receipt.deliveries)) {
      throw fail(`Malformed local PTY owner ${command} packet`, 'NATIVE_AUDIT_INCOMPLETE');
    }
    // Packet schema is audit-correlated, while the physical command ack is
    // command-correlated. The producer currently does not stamp command IDs
    // into fixed-name packets; accept only after reading the matching ack.
    if (!owner) owner = receipt.owner;
    if (receipt.owner.ownerId !== owner.ownerId || receipt.owner.pid !== owner.pid ||
        receipt.owner.executable !== owner.executable || receipt.owner.processStart !== owner.processStart ||
        receipt.owner.component !== owner.component) throw fail('Local PTY audit packet changed owner identity', 'NATIVE_AUDIT_INCOMPLETE');
    if (command === 'close' && (receipt.complete !== true ||
        !Number.isSafeInteger(receipt.endSeq) || receipt.endSeq < receipt.baselineSeq)) {
      throw fail('Local PTY owner close packet does not prove all request deliveries completed', 'NATIVE_AUDIT_INCOMPLETE');
    }
    if (command === 'close') openCompleted = false;
    return { ack, packet: receipt, instanceId, commandSeq };
  }
  return {
    identity: (args, signal) => enqueue(() => request('identity', args, signal)),
    open: (args, signal) => enqueue(() => request('open', args, signal)),
    read: (args, signal) => enqueue(() => request('read', args, signal)),
    close: (args, signal) => enqueue(() => request('close', args, signal)),
    async dispose() { closed = true; await pending; },
    get owner() { return owner ? { ...owner, instanceId } : null; },
    get runId() { return runId; },
    get auditId() { return auditId; },
    get nextCommandSeq() { return commandSequence + 1; },
    commandCursor() { return { lastCommandId: lastCommandId ?? null, nextCommandSeq: commandSequence + 1 }; },
  };
}

function createNativeBarrierAdapter({ hub, emitReceipt }) {
  const path = hub.receiptPath('presentation');
  let chain = Promise.resolve();
  let cursor = -1;
  let closed = false;
  let failure;
  const records = [];
  const pending = new Set();
  const controlWaits = new Set();
  const consume = async () => {
    let text;
    try { text = await readFile(path, 'utf8'); } catch (error) { if (error.code === 'ENOENT') return; throw error; }
    const lines = text.split('\n');
    lines.pop(); // A partial append is not a settled physical line.
    if (lines.length < cursor + 1) throw fail('Presentation receipt producer truncated evidence');
    for (let index = cursor + 1; index < lines.length; index++) {
      if (lines[index].trim()) {
        const raw = correlateReceipt(JSON.parse(lines[index]), hub, 'native presentation');
        requireSevenTupleReceipt(raw);
        if (raw.presented !== true || !Number.isSafeInteger(raw.producerPid) || raw.producerPid <= 0) {
          throw fail('Presentation lacks accepted frame or positive producer PID');
        }
        const record = { raw, path, lineIndex: index };
        records.push(record);
        emitReceipt(record);
        for (const waiter of [...pending]) waiter.receive(record);
      }
      cursor = index;
    }
  };
  const enqueue = () => {
    chain = chain.then(() => { if (!closed) return consume(); }).catch((error) => {
      failure = error;
      for (const waiter of [...pending]) waiter.reject(error);
      output({ event: 'receipt-error', error: error.message });
    });
    return chain;
  };
  const watcher = watch(hub.dir, (_, name) => { if (String(name) === 'presentation.receipt.jsonl') enqueue(); });
  enqueue(); // Subscription precedes the initial read and all launches.
  async function control(name, suffix, method, timeoutMs, signal) {
    if (closed) throw fail('Native barrier adapter closed');
    const deadline = Date.now() + timeoutMs;
    await new Promise((resolveWait, reject) => {
      let fileWatcher;
      let timer;
      const dispose = () => { fileWatcher?.close(); clearTimeout(timer); signal.removeEventListener('abort', abort); controlWaits.delete(abort); };
      const finish = (error) => { dispose(); error ? reject(error) : resolveWait(); };
      const abort = () => finish(signal.reason ?? fail('Native barrier adapter closed'));
      const inspect = async () => {
        try { await readFile(join(hub.dir, `${name}.${suffix}.json`)); finish(); }
        catch (error) { if (error.code !== 'ENOENT') finish(error); }
      };
      fileWatcher = watch(hub.dir, (_, file) => { if (String(file) === `${name}.${suffix}.json`) void inspect(); });
      controlWaits.add(abort);
      signal.addEventListener('abort', abort, { once: true });
      timer = setTimeout(() => finish(fail(`Barrier ${name} ${suffix} deadline`, 'BARRIER_ACK_TIMEOUT')), timeoutMs);
      if (signal.aborted) abort(); else void inspect();
    });
    if (closed || signal.aborted) throw signal.reason ?? fail('Native barrier adapter closed');
    return hub[method](name, Math.max(1, deadline - Date.now()));
  }
  return {
    prearm: (name, options) => hub.prearm(name, options),
    awaitRegistered: (name, timeoutMs, signal) => control(name, 'armed-ack', 'awaitRegistered', timeoutMs, signal),
    awaitHeld: (name, timeoutMs, signal) => control(name, 'held', 'awaitHeld', timeoutMs, signal),
    bindBackendSession: (name, sessionId) => hub.bindBackendSession(name, sessionId),
    awaitBound: (name, timeoutMs, signal) => control(name, 'bound-ack', 'awaitBound', timeoutMs, signal),
    release: (name) => hub.release(name),
    async cursor() { await enqueue(); if (failure) throw failure; return { path, afterLine: cursor }; },
    async drain() { await enqueue(); if (failure) throw failure; },
    awaitPresentation({ sessionId, expected, afterLine, timeoutMs, producerPid }, signal) {
      requireSevenTupleReceipt(expected, {}, 'expected native presentation tuple');
      if (expected.backendSessionId !== sessionId || !Number.isSafeInteger(afterLine) || afterLine < -1 ||
          !Number.isSafeInteger(producerPid) || producerPid <= 0 || !Number.isFinite(timeoutMs) || timeoutMs <= 0) {
        throw fail('Presentation requires exact session, cursor, remaining deadline and spawn-recorded GUI PID');
      }
      return new Promise((resolveWait, reject) => {
        let timer;
        const dispose = () => { pending.delete(waiter); clearTimeout(timer); signal.removeEventListener('abort', abort); };
        const waiter = {
          reject(error) { dispose(); reject(error); },
          receive(record) {
            if (record.lineIndex <= afterLine || record.raw.producerPid !== producerPid) return;
            if (!ATTACH_TUPLE_FIELDS.every((field) => record.raw.attachTuple[field] === expected[field])) return;
            dispose(); resolveWait(record);
          },
        };
        const abort = () => waiter.reject(signal.reason);
        pending.add(waiter);
        signal.addEventListener('abort', abort, { once: true });
        timer = setTimeout(() => waiter.reject(fail('Native presentation deadline', 'BARRIER_ACK_TIMEOUT')), timeoutMs);
        if (closed || failure || signal.aborted) waiter.reject(failure ?? signal.reason ?? fail('Native barrier adapter closed'));
        else for (const record of records) { if (!pending.has(waiter)) break; waiter.receive(record); }
      });
    },
    close() {
      closed = true; watcher.close();
      for (const waiter of [...pending]) waiter.reject(fail('Native barrier adapter closed'));
      for (const abort of [...controlWaits]) abort();
    },
  };
}

async function dispatch(method, args, signal) {
  switch (method) {
    case 'prepare': {
      const { worktreeRoot, worktree, runId, operationId, binary, uiDist, manifest } = args;
      if (!/^[a-z0-9-]+$/.test(runId) || !inside(worktreeRoot, worktree)) throw fail('Noncanonical worktree/runId');
      if (typeof operationId !== 'string' || !operationId) throw fail('Missing presentation operation identity');
      const actualRoot = await command('git', ['-C', worktree, 'rev-parse', '--show-toplevel'], signal);
      if (resolve(actualRoot) !== resolve(worktree)) throw fail('Remote path is not the selected git worktree');
      const linked = await command('git', ['-C', worktree, 'rev-parse', '--git-dir'], signal);
      if (linked === '.git') throw fail('QA checkout must be a linked canonical git worktree');
      if (!inside(worktree, binary) || !inside(worktree, uiDist)) throw fail('Candidate artifacts outside checkout');
      const built = JSON.parse(await readFile(manifest, 'utf8'));
      const binarySha256 = await hashFile(binary);
      const uiSha256 = await hashTree(uiDist);
      if (built.binarySha256 !== binarySha256 || built.uiSha256 !== uiSha256 || !built.stamp ||
        !built.sourcePatchSha256 || !built.baseRevision) throw fail('Artifact build manifest does not match measured hashes');
      identityProbeBinary = await realpath(binary);
      const isWindows = process.platform === 'win32';
      const wrapperPath = resolve(worktree, isWindows ? 'scripts/qa/fixtures/fixture-pty-child.cmd' : 'scripts/qa/fixtures/fixture-pty-child.sh');
      if (!(await stat(wrapperPath)).isFile()) throw fail('Fixture PTY wrapper is not a file');
      const profile = join(worktree, '.native-qa', runId);
      await mkdir(join(worktree, '.native-qa'), { recursive: true });
      await mkdir(profile, { recursive: false });
      for (const name of ['data', 'runtime', 'session', 'home', 'evidence', 'receipts', 'tmp']) await mkdir(join(profile, name));
      if (!isWindows) { await chmod(wrapperPath, 0o755).catch(() => {}); }
      profiles.add(profile);
      frontendProfiles.set(profile, { worktree, uiDist });
      const hub = new BarrierHub(profile, { runId, operationId });
      const receiptsDir = join(profile, 'receipts');
      let readyResolver;
      const readyPromise = new Promise((res) => { readyResolver = res; });
      const readyWatcher = watch(receiptsDir, async (eventType, filename) => {
        if (filename === 'fixture-child-ready.json' || !filename) {
          try {
            const raw = JSON.parse(await readFile(join(receiptsDir, 'fixture-child-ready.json'), 'utf8'));
            if (raw?.status === 'ready' && raw.endpoint) { readyWatcher.close(); readyResolver(raw); }
          } catch (_) {}
        }
      });
      fixtureReadyWatches.set(profile, { watcher: readyWatcher, promise: readyPromise });
      barriers.set(profile, createNativeBarrierAdapter({ hub, emitReceipt: ({ raw, path, lineIndex }) =>
        output({ event: 'receipt', name: 'presentation.receipt.jsonl', raw, path, lineIndex }) }));
      return { worktree, profile, store: join(profile, 'data'), socket: join(profile, 'runtime', process.platform === 'win32' ? 'daemon.port' : 'daemon.sock'),
        candidate: built.stamp, binarySha256, uiSha256, productionUntouched: true, binary, uiDist,
        env: { FERRYX_QA_ISOLATED: '1', FERRYX_DATA_DIR: join(profile, 'data'), FERRYX_RUNTIME_DIR: join(profile, 'runtime'),
          FERRYX_SESSION_DIR: join(profile, 'session'), ...hub.env(), FERRYX_QA_SHELL: wrapperPath,
          FERRYX_QA_PROFILE: profile, FERRYX_QA_RUN_ID: runId, FERRYX_QA_OPERATION_ID: operationId,
          HOME: join(profile, 'home'), USERPROFILE: join(profile, 'home'),
          // The debug GUI writes its switch-debug trace under the process temp dir;
          // keep it inside the owned profile so collection never reads shared temp.
          TMP: join(profile, 'tmp'), TEMP: join(profile, 'tmp'), TMPDIR: join(profile, 'tmp'),
          // WebView2 keeps its profile under %LOCALAPPDATA%, which USERPROFILE does not move;
          // without this the candidate GUI restores another install's localStorage.
          ...(process.platform === 'win32' ? { WEBVIEW2_USER_DATA_FOLDER: join(profile, 'webview') } : {}) } };
    }
    case 'start-frontend': {
      if (!profiles.has(args.profile)) throw fail('Unknown frontend profile');
      const entry = frontendProfiles.get(args.profile);
      if (entry.resource) throw fail('Frontend setup already requested');
      entry.resource = createRemoteFrontend();
      return entry.resource.start({ ...entry, frontend: args.frontend, signal });
    }
    case 'spawn-daemon': {
      if (!profiles.has(args.setup?.profile)) throw fail('Unknown daemon profile');
      return launchNativeDaemon({ ...args, signal, binary: args.setup.binary });
    }
    case 'provision-gui-launch': {
      const { setup, hostId, runId, operationId } = args;
      if (!profiles.has(setup.profile)) throw fail('Unknown launcher profile');
      const specPath = join(setup.profile, 'gui-launch-spec.json');
      await persistLaunch(specPath, { worktreeRoot: args.worktreeRoot, worktree: setup.worktree,
        profile: setup.profile, binary: setup.binary, binarySha256: setup.binarySha256,
        runId, operationId, candidate: setup.candidate, hostId, env: setup.env,
        receiptPath: join(setup.profile, 'receipts', 'gui-launch.json') });
      if (process.platform !== 'win32') return { specPath };
      // MahoRD typing on Windows drops '-' and '_' (measured), so the Run dialog receives only the
      // 8.3 short path of a launcher whose full command line lives in the file.
      const launcher = join(setup.profile, 'launch.cmd');
      await mkdir(join(setup.profile, 'evidence'), { recursive: true });
      const entryLog = join(setup.profile, 'evidence', 'launcher-entry.log');
      const outputLog = join(setup.profile, 'evidence', 'launcher-output.log');
      const exitLog = join(setup.profile, 'evidence', 'launcher-exit.log');
      const cmdBody = [
        '@echo off',
        `> "${entryLog}" echo launcher-entry %DATE% %TIME%`,
        `@"${process.execPath}" "${resolve(process.argv[1])}" --launch-gui "${specPath}" > "${outputLog}" 2>&1`,
        'set "EXIT_CODE=%ERRORLEVEL%"',
        `> "${exitLog}" echo %EXIT_CODE%`,
        'exit /b %EXIT_CODE%',
        '',
      ].join('\r\n');
      await writeFile(launcher, cmdBody, 'utf8');
      const launcherShortPath = (await command('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
        `(New-Object -ComObject Scripting.FileSystemObject).GetFile('${launcher.replaceAll("'", "''")}').ShortPath`], signal)).trim();
      if (!launcherShortPath || /[-_]/.test(launcherShortPath)) throw fail(`Launcher short path is not typeable: ${launcherShortPath}`);
      return { specPath, launcherShortPath };
    }
    case 'await-gui-launch': {
      if (!profiles.has(args.profile)) throw fail('Unknown launcher profile');
      const path = join(args.profile, 'receipts', 'gui-launch.json');
      return new Promise((resolveWait, reject) => {
        let timer;
        const dispose = () => { watcher.close(); clearTimeout(timer); signal.removeEventListener('abort', abort); };
        const abort = () => { dispose(); reject(signal.reason); };
        const inspect = async () => {
          try {
            const raw = JSON.parse(await readFile(path, 'utf8'));
            dispose();
            // The GUI is spawned by the separate --launch-gui launcher process, so
            // its spawn-time receipt reaches this agent only through this file.
            // Register it in the profile roster (exact PID/executable/start)
            // so the audit census sees the real GUI owner.
            if (raw?.role === 'gui' && raw.status === 'spawned' && Number.isSafeInteger(raw.pid) && raw.pid > 0 &&
                raw.executable && raw.processStart) {
              const roster = spawnOwned.get(args.profile) ?? new Map();
              roster.set(`gui:${raw.runId}:${raw.pid}`, raw);
              spawnOwned.set(args.profile, roster);
            }
            resolveWait(raw);
          }
          catch (error) { if (error.code !== 'ENOENT') { dispose(); reject(error); } }
        };
        const watcher = watch(join(args.profile, 'receipts'), (_, name) => { if (String(name) === 'gui-launch.json') void inspect(); });
        output({ event: 'gui-launch-subscribed', profile: args.profile });
        signal.addEventListener('abort', abort, { once: true });
        timer = setTimeout(() => { dispose(); reject(fail('GUI launcher receipt deadline')); }, args.timeoutMs);
        if (signal.aborted) abort(); else void inspect();
      });
    }
    case 'setup-target': return inspectSetupTarget({ ...args, signal });
    case 'setup-child-windows': return inspectSetupChildWindows({ ...args, signal });
    case 'probe': return probe(args.pid, signal);
    case 'exit': {
      const live = await probe(args.pid, signal);
      if (live.exited) return { pid: args.pid, processStart: args.processStart, exited: true };
      if (live.processStart !== args.processStart) throw fail('PID incarnation changed before exit subscription');
      const owned = children.get(args.pid);
      if (owned) await Promise.race([owned.exit, new Promise((_, reject) => signal.addEventListener('abort', () => reject(signal.reason), { once: true }))]);
      else if (process.platform === 'win32') {
        await waitWindowsExactPidExit(args.pid, args.processStart, signal);
      } else {
        // pidfd is an exact event subscription, not /proc polling. Recheck start
        // after opening pidfd so PID reuse during subscription is refused.
        await command('python3', ['-c',
          'import os,select,sys\npid=int(sys.argv[1]); fd=os.pidfd_open(pid); s=open("/proc/%s/stat"%pid).read(); assert s[s.rfind(")")+2:].split()[19]==sys.argv[2]; select.select([fd],[],[]); os.close(fd)',
          String(args.pid), args.processStart], signal);
      }
      return { pid: args.pid, processStart: args.processStart, exited: true };
    }
    case 'terminate': {
      const live = await probe(args.pid, signal);
      if (live.exited) return { exited: true };
      if (live.processStart !== args.processStart || resolve(live.executable) !== resolve(args.executable)) throw fail('Exact PID identity mismatch');
      if (process.platform === 'win32') await command('taskkill', ['/PID', String(args.pid), '/F'], signal);
      else process.kill(args.pid, 'SIGTERM');
      return { signalled: true };
    }
    case 'worktrees': return command('git', ['-C', args.canonicalClone, 'worktree', 'list'], signal);
    case 'collect-launch-evidence': return collectLaunchEvidence(args.profile, signal);
    case 'remove-profile': {
      if (!profiles.has(args.profile)) throw fail('Unowned profile removal refused');
      const worker = activeProbeWorkers.get(args.profile);
      if (worker) {
        activeProbeWorkers.delete(args.profile);
        const workerDir = join(args.profile, 'evidence', 'setup-probes', 'worker-queue');
        try {
          await persistLaunch(join(workerDir, 'zzzz-stop.req.json'), { stop: true });
          await Promise.race([worker.subscribed.promise, new Promise((r) => setTimeout(r, 5000))]);
        } catch {} finally {
          worker.subscribed.close();
          try { await command('schtasks', worker.deleteArgs, signal); } catch {}
          const tasks = pendingSetupTasks.get(args.profile);
          if (tasks) {
            tasks.delete(worker.taskName);
            if (tasks.size === 0) pendingSetupTasks.delete(args.profile);
          }
        }
      }
      const retained = pendingSetupTasks.get(args.profile);
      if (retained?.size) {
        throw fail(`Setup probe task retained: ${[...retained].join(', ')}; profile preserved; no process killed`);
      }
      await frontendProfiles.get(args.profile)?.resource?.close(); frontendProfiles.delete(args.profile);
      await localOwnerRpcs.get(args.profile)?.dispose(); localOwnerRpcs.delete(args.profile);
      barriers.get(args.profile)?.close(); barriers.delete(args.profile);
      await rm(args.profile, { recursive: true }); profiles.delete(args.profile);
      setupGuiReceipts.delete(args.profile);
      return { removed: args.profile };
    }
    case 'snapshot': {
      // Correlated request: the GUI's QA snapshot thread picks up
      // barriers/native-snapshot-<id>.request.json, asks the frontend for the
      // exact target pane, and settles native-snapshot-<id>.response.json itself
      // (success or typed refusal, including SNAPSHOT_TIMEOUT). The watch is
      // installed before the request is written; the agent never synthesizes a
      // snapshot and passes refusals through verbatim.
      if (args.requestId === undefined) {
        // Legacy diagnostic form: last-writer receipt only.
        try { return JSON.parse(await readFile(join(args.profile, 'receipts', 'native-current.json'), 'utf8')); }
        catch (error) { if (error.code === 'ENOENT') throw fail('Required product producer writeNativeRecoverySnapshot(profile, currentTuple, target, focus, paneBounds) is absent'); throw error; }
      }
      return requestCorrelatedSnapshot(args);
    }
    case 'ownership-subscribe': {
      if (!profiles.has(args.profile)) throw fail('Unknown ownership profile');
      if (ownershipStreams.has(args.profile)) throw fail('Ownership stream already subscribed');
      const stream = createOwnedPtyStream({ path: join(args.profile, 'barriers', 'owned-pty-creation.receipt.jsonl'),
        onError: (error) => output({ event: 'receipt-error', error: error.message }),
        emit: (record) => output({ event: 'receipt', name: 'owned-pty-creation.receipt.jsonl', ...record }) });
      ownershipStreams.set(args.profile, stream);
      return stream.cursor();
    }
    case 'local-owner-identity':
    case 'local-owner-open':
    case 'local-owner-read':
    case 'local-owner-close': {
      if (!profiles.has(args.profile)) throw fail('Unknown local owner profile', 'NATIVE_AUDIT_INCOMPLETE');
      let rpc = localOwnerRpcs.get(args.profile);
      if (!rpc) {
        rpc = createLocalPtyOwnerRpc({ profile: args.profile, runId: args.runId, operationId: args.operationId,
          ownerCommandId: () => randomUUID() });
        localOwnerRpcs.set(args.profile, rpc);
      }
      const ownerAction = method === 'local-owner-identity' ? 'identity' : method === 'local-owner-open' ? 'open' : method === 'local-owner-read' ? 'read' : 'close';
      const result = await rpc[ownerAction](args, signal);
      return result;
    }
    case 'local-owner-command-cursor': {
      const rpc = localOwnerRpcs.get(args.profile);
      return rpc ? rpc.commandCursor() : { lastCommandId: null, nextCommandSeq: 1 };
    }
    case 'owned-process-census': {
      if (!profiles.has(args.profile)) throw fail('Unknown owner census profile', 'NATIVE_AUDIT_INCOMPLETE');
      const receipts = [...(spawnOwned.get(args.profile)?.values() ?? [])];
      if (!receipts.some((receipt) => receipt.role === 'gui') || !receipts.some((receipt) => receipt.role === 'daemon'))
        throw fail('Spawn-owned GUI and daemon census is not closed yet', 'NATIVE_AUDIT_INCOMPLETE');
      const census = receipts.map((receipt) => ({ ownerId: `${receipt.role}:${receipt.runId}:${receipt.pid}`,
        pid: receipt.pid, executable: receipt.executable, processStart: receipt.processStart, component: receipt.role }));
      const local = localOwnerRpcs.get(args.profile)?.owner;
      if (local && receipts.some((receipt) => receipt.role === 'daemon' && receipt.pid === local.pid &&
          receipt.executable === local.executable && receipt.processStart === local.processStart)) {
        census.push({ ownerId: local.ownerId, pid: local.pid, executable: local.executable,
          processStart: local.processStart, component: local.component });
      }
      return census;
    }
    case 'audit-producer-roster': {
      if (!profiles.has(args.profile)) throw fail('Unknown audit producer roster profile', 'NATIVE_AUDIT_INCOMPLETE');
      const receipts = [...(spawnOwned.get(args.profile)?.values() ?? [])];
      const producers = [];
      const gui = receipts.find((r) => r.role === 'gui');
      if (gui) {
        producers.push({ ownerId: `gui:${gui.runId}:${gui.pid}`,
          pid: gui.pid, executable: gui.executable, processStart: gui.processStart, component: 'gui' });
      }
      const daemon = receipts.find((r) => r.role === 'daemon');
      if (daemon) {
        producers.push({ ownerId: `daemon:${daemon.runId}:${daemon.pid}`,
          pid: daemon.pid, executable: daemon.executable, processStart: daemon.processStart, component: 'daemon' });
      }
      const local = localOwnerRpcs.get(args.profile)?.owner;
      if (local) {
        producers.push({ ownerId: local.ownerId, pid: local.pid, executable: local.executable,
          processStart: local.processStart, component: local.component });
      }
      if (producers.length === 0) throw fail('No scenario owner producers recorded', 'NATIVE_AUDIT_INCOMPLETE');
      return producers;
    }
    case 'owned-process-roster': {
      if (!profiles.has(args.profile)) throw fail('Unknown owner roster profile', 'NATIVE_AUDIT_INCOMPLETE');
      const receipts = [...(spawnOwned.get(args.profile)?.values() ?? [])];
      if (!receipts.some((receipt) => receipt.role === 'gui') || !receipts.some((receipt) => receipt.role === 'daemon'))
        throw fail('Spawn-owned GUI and daemon roster is not closed yet', 'NATIVE_AUDIT_INCOMPLETE');
      const census = receipts.map((receipt) => ({ ownerId: `${receipt.role}:${receipt.runId}:${receipt.pid}`,
        pid: receipt.pid, executable: receipt.executable, processStart: receipt.processStart, component: receipt.role }));
      const local = localOwnerRpcs.get(args.profile)?.owner;
      if (!local || !receipts.some((receipt) => receipt.role === 'daemon' && receipt.pid === local.pid &&
          receipt.executable === local.executable && receipt.processStart === local.processStart))
        throw fail('Local PTY producer identity does not match the spawn-recorded daemon', 'NATIVE_AUDIT_INCOMPLETE');
      census.push({ ownerId: local.ownerId, pid: local.pid, executable: local.executable,
        processStart: local.processStart, component: local.component });
      const roster = [...census];
      roster.census = census;
      roster.producers = census;
      return roster;
    }
    case 'audit-owner': {
      if (!profiles.has(args.profile)) throw fail('Unknown local audit profile', 'NATIVE_AUDIT_INCOMPLETE');
      if (args.component === 'terminal-local-pty') {
        const rpc = localOwnerRpcs.get(args.profile);
        if (!rpc || rpc.auditId !== args.auditId) throw fail('Local PTY owner channel has not been opened', 'NATIVE_AUDIT_INCOMPLETE');
        const owner = rpc.owner;
        if (owner && (owner.ownerId !== args.ownerId || owner.pid !== args.pid || owner.executable !== args.executable ||
            owner.processStart !== args.processStart || owner.component !== args.component)) {
          throw fail('Local PTY owner does not match spawn-owned roster', 'NATIVE_AUDIT_INCOMPLETE');
        }
        if (args.commandSeq !== rpc.nextCommandSeq || !Number.isSafeInteger(args.commandSeq))
          throw fail('Local PTY owner command sequence is stale or out of order', 'NATIVE_AUDIT_INCOMPLETE');
        if (args.action === 'read') {
          const result = await rpc.read(args, signal);
          if (result.packet.owner.ownerId !== args.ownerId || result.packet.owner.pid !== args.pid ||
              result.packet.owner.executable !== args.executable || result.packet.owner.processStart !== args.processStart ||
              result.packet.owner.component !== args.component)
            throw fail('Local PTY read owner differs from frozen identity', 'NATIVE_AUDIT_INCOMPLETE');
          if (result.commandSeq !== args.commandSeq) throw fail('Local PTY read acknowledgment sequence mismatch', 'NATIVE_AUDIT_INCOMPLETE');
          return result;
        }
        if (args.action === 'close') {
          const result = await rpc.close(args, signal);
          if (result.commandSeq !== args.commandSeq) throw fail('Local PTY close acknowledgment sequence mismatch', 'NATIVE_AUDIT_INCOMPLETE');
          return result;
        }
        throw fail('Unknown local owner audit action', 'NATIVE_AUDIT_INCOMPLETE');
      }

      // Process producer (not agent) owns open/read/close commandSeq ack + independent ingress/delivery tokens
      const barrierDir = join(args.profile, 'barriers');
      const reqFile = join(barrierDir, `${args.component}-owner-${args.action}.request.json`);
      const reqPayload = {
        action: args.action,
        runId: args.runId,
        operationId: args.operationId,
        auditId: args.auditId,
        ownerId: args.ownerId,
        pid: args.pid,
        commandSeq: args.commandSeq,
        baselineSeq: args.baselineSeq ?? 0,
      };
      await writeFile(reqFile, JSON.stringify(reqPayload, null, 2));

      // Wait for the producer process to write its correlated command acknowledgment:
      const ackFile = join(barrierDir, `${args.component}-owner-${args.action}.ack.${args.commandSeq}.json`);
      let ack;
      const deadline = Date.now() + 2500;
      while (Date.now() < deadline) {
        try {
          ack = JSON.parse(await readFile(ackFile, 'utf8'));
          if (ack && ack.accepted === true) break;
        } catch (_) {}
        await new Promise((r) => setTimeout(r, 20));
      }
      if (!ack || ack.accepted !== true) {
        throw fail(`Producer ${args.component} did not write correlated command acknowledgment`, 'NATIVE_AUDIT_INCOMPLETE');
      }

      // Read the physical audit packet written by the producer process:
      const packetFile = join(barrierDir, `${args.component}-owner-audit.json`);
      let packet;
      try {
        packet = JSON.parse(await readFile(packetFile, 'utf8'));
      } catch (err) {
        throw fail(`Missing actual ${args.component} audit packet from producer: ${err.message}`, 'NATIVE_AUDIT_INCOMPLETE');
      }

      if (!packet || packet.auditId !== args.auditId ||
          packet.owner?.ownerId !== args.ownerId || packet.owner?.component !== args.component) {
        throw fail(`Producer ${args.component} audit packet identity mismatch`, 'NATIVE_AUDIT_INCOMPLETE');
      }
      if (args.action === 'close' && packet.complete !== true) {
        throw fail(`Producer ${args.component} audit packet incomplete on close`, 'NATIVE_AUDIT_INCOMPLETE');
      }

      return {
        packet,
        commandSeq: args.commandSeq,
        ack,
      };
    }
    case 'ownership-drain': {
      const stream = ownershipStreams.get(args.profile);
      if (!stream) throw fail('Ownership stream not subscribed before launch');
      await stream.drain();
      return { drained: true };
    }
    case 'watch-receipts': {
      const directory = join(args.profile, 'receipts');
      let chain = Promise.resolve();
      const offsets = new Map();
      const partials = new Map();
      const consume = async (name) => {
        if (!['native-events.jsonl', 'native-spawn.jsonl'].includes(name)) return;
        const path = join(directory, name);
        let bytes;
        try { bytes = await readFile(path); } catch (error) { if (error.code === 'ENOENT') return; throw error; }
        const offset = offsets.get(name) ?? 0;
        if (bytes.length < offset) throw fail('Receipt producer truncated evidence');
        offsets.set(name, bytes.length);
        const lines = ((partials.get(name) ?? '') + bytes.subarray(offset).toString('utf8')).split('\n');
        partials.set(name, lines.pop());
        for (const line of lines) if (line.trim()) output({ event: 'receipt', name, raw: JSON.parse(line) });
      };
      const enqueue = (name) => { chain = chain.then(() => consume(name)).catch((error) => output({ event: 'receipt-error', error: error.message })); };
      // Watch before initial read: notifications and snapshot reads serialize.
      const watcher = watch(directory, (_, name) => { if (name) enqueue(String(name)); });
      watches.set(args.profile, { watcher, drain: () => chain });
      for (const name of ['native-events.jsonl', 'native-spawn.jsonl']) enqueue(name);
      await chain;
      return { subscribed: true };
    }
    case 'barrier-prearm': return barriers.get(args.profile).prearm(args.name, args.options);
    case 'barrier-registered': return barriers.get(args.profile).awaitRegistered(args.name, args.timeoutMs, signal);
    case 'barrier-held': return barriers.get(args.profile).awaitHeld(args.name, args.timeoutMs, signal);
    case 'barrier-bind': return barriers.get(args.profile).bindBackendSession(args.name, args.sessionId);
    case 'barrier-bound': return barriers.get(args.profile).awaitBound(args.name, args.timeoutMs, signal);
    case 'barrier-release': return barriers.get(args.profile).release(args.name);
    case 'barrier-cursor': return barriers.get(args.profile).cursor();
    case 'barrier-presentation': return barriers.get(args.profile).awaitPresentation(args, signal);
    case 'drain': { await watches.get(args.profile)?.drain(); await barriers.get(args.profile)?.drain(); return { drained: true }; }
    case 'start-forwarder-proxy': {
      if (forwarderTransports.has(args.profile)) {
        await forwarderTransports.get(args.profile).release();
        forwarderTransports.delete(args.profile);
      }
      const { NativeForwardingTransport } = await import('./native-forwarding-transport.mjs');
      const transport = new NativeForwardingTransport({
        ctx: { setup: { profile: args.profile } },
      });
      forwarderTransports.set(args.profile, transport);
      transport.subscribe((event) => {
        output({ event: 'forwarder-receipt', profile: args.profile, ...event });
      });
      const endpoint = await transport.startProxy({
        upstreamSocket: args.upstreamSocket,
        upstreamPort: args.upstreamPort,
        signal,
      });
      return { endpoint };
    }
    case 'arm-forwarder-lost-ack': {
      const transport = forwarderTransports.get(args.profile);
      if (!transport) throw fail('Forwarder proxy transport is not running');
      return transport.armLostAcknowledgment(args);
    }
    case 'end-forwarder-stream': {
      const transport = forwarderTransports.get(args.profile);
      if (!transport) throw fail('Forwarder proxy transport is not running');
      return transport.endCurrentStream(args);
    }
    case 'disconnect-forwarder': {
      const transport = forwarderTransports.get(args.profile);
      if (!transport) throw fail('Forwarder proxy transport is not running');
      return transport.disconnect(args);
    }
    case 'hold-forwarder-outage': {
      const transport = forwarderTransports.get(args.profile);
      if (!transport) throw fail('Forwarder proxy transport is not running');
      return transport.holdOutage(args);
    }
    case 'restore-forwarder': {
      const transport = forwarderTransports.get(args.profile);
      if (!transport) throw fail('Forwarder proxy transport is not running');
      return transport.restore(args);
    }
    case 'release-forwarder': {
      const transport = forwarderTransports.get(args.profile);
      if (transport) {
        await transport.release();
        forwarderTransports.delete(args.profile);
      }
      return { released: true };
    }
    case 'await-fixture-child-ready': {
      if (!profiles.has(args.profile)) throw fail('Unknown profile for fixture child');
      const item = fixtureReadyWatches.get(args.profile);
      const readyFile = join(args.profile, 'receipts', 'fixture-child-ready.json');
      let raw;
      try {
        raw = JSON.parse(await readFile(readyFile, 'utf8'));
      } catch (_) {
        if (!item) throw fail('No fixture ready subscription found for profile');
        const timeoutMs = args.timeoutMs ?? 30_000;
        raw = await Promise.race([
          item.promise,
          new Promise((_, rej) => setTimeout(() => rej(fail('Fixture child readiness deadline expired')), timeoutMs)),
          new Promise((_, rej) => { if (signal) signal.addEventListener('abort', () => rej(signal.reason), { once: true }); }),
        ]);
      }
      if (!raw || raw.status !== 'ready' || !raw.endpoint) throw fail('Malformed fixture child ready receipt');
      fixtureEndpoints.set(args.profile, raw);
      return raw;
    }
    case 'fixture-emit-marker': {
      const info = fixtureEndpoints.get(args.profile);
      if (!info?.endpoint) throw fail('Fixture child is not ready or endpoint unknown');
      const isWindows = process.platform === 'win32';
      const client = isWindows
        ? createConnection({ host: '127.0.0.1', port: info.endpoint.port })
        : createConnection({ path: info.endpoint.path });
      return new Promise((resolveWait, rejectWait) => {
        const decoder = new StringDecoder('utf8');
        let buf = '';
        const timer = setTimeout(() => {
          client.destroy();
          rejectWait(fail('Fixture child emit marker timeout'));
        }, args.timeoutMs ?? 10_000);
        client.on('error', (err) => { clearTimeout(timer); rejectWait(err); });
        client.on('data', (d) => {
          buf += decoder.write(d);
          const nl = buf.indexOf('\n');
          if (nl !== -1) {
            const line = buf.slice(0, nl);
            try {
              const res = JSON.parse(line);
              if (res.type === 'marker-emitted') { clearTimeout(timer); client.destroy(); resolveWait(res); }
              else if (res.type === 'error') {
                clearTimeout(timer); client.destroy(); rejectWait(fail(`Fixture child error: ${res.code ?? res.error}`));
              }
            } catch (_) {}
          }
        });
        client.write(JSON.stringify({
          type: 'emit-marker', marker: args.marker, runId: args.runId, operationId: args.operationId
        }) + '\n');
      });
    }
    case 'unwatch': {
      await frontendProfiles.get(args.profile)?.resource?.close();
      ownershipStreams.get(args.profile)?.close(); ownershipStreams.delete(args.profile);
      watches.get(args.profile)?.watcher.close(); watches.delete(args.profile);
      fixtureReadyWatches.get(args.profile)?.watcher.close(); fixtureReadyWatches.delete(args.profile);
      barriers.get(args.profile)?.close(); barriers.delete(args.profile);
      if (forwarderTransports.has(args.profile)) {
        await forwarderTransports.get(args.profile).release();
        forwarderTransports.delete(args.profile);
      }
      return { closed: true };
    }
    default: throw fail(`Unknown native agent method ${method}`);
  }
}

if (process.argv.includes('--read-only-setup-probe')) {
  const specPath = process.argv[process.argv.indexOf('--read-only-setup-probe') + 1];
  const { gui, script, ownerPath, completionPath } = JSON.parse(await readFile(specPath, 'utf8'));
  if (process.platform !== 'win32' || !Number.isSafeInteger(gui?.pid) || gui.pid <= 0) throw fail('Invalid Windows setup probe target');
  // This entry point receives only harness-generated read-only scripts. Identity/session
  // checks and enumeration execute in the SAME PowerShell process on the interactive task.
  const signal = new AbortController().signal;
  const scriptPath = join(resolve(specPath, '..'), 'probe-script.ps1');
  const precompiledDll = 'C:\\Users\\sook\\ferryx-wt\\.evidence\\recovery-task9-win-20261017\\FerryxQaWin32.dll';
  let fastScript = script;
  try {
    await stat(precompiledDll);
    fastScript = script.replace(/Add-Type @"\r?\nusing System;[\s\S]*?"@;/g, `Add-Type -Path '${precompiledDll}';`);
  } catch {}
  const ownerEmitLine = `\$__op = Get-Process -Id ${process.pid}; @{ probe = 'setup-probe-owner'; pid = ${process.pid}; exited = \$false; executable = \$__op.Path; processStart = \$__op.StartTime.ToUniversalTime().Ticks.ToString(); ppid = ${process.ppid} } | ConvertTo-Json -Compress;`;
  await writeFile(scriptPath, `${ownerEmitLine}\r\n${buildSetupProbeSessionScript(gui)}\r\n${fastScript}\r\n`, { encoding: 'utf8', mode: 0o600 });
  let owner = { pid: process.pid, exited: false, executable: process.execPath, processStart: String(Date.now()), ppid: process.ppid };
  let result;
  try {
    const rawStdout = await command('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File',
      scriptPath], signal);
    const parsedOwner = parseWindowsProbeLine(rawStdout, 'setup-probe-owner');
    if (parsedOwner.ok) {
      const { probe: _p, ...restOwner } = parsedOwner.probe;
      owner = restOwner;
    }
    await persistLaunch(ownerPath, owner);
    result = { owner, stdout: rawStdout };
  } catch (error) {
    await persistLaunch(ownerPath, owner);
    result = { owner, error: error.message };
    process.exitCode = 1;
  }
  await persistLaunch(completionPath, result);
  const workerDir = join(resolve(specPath, '..', '..'), 'worker-queue');
  try {
    await mkdir(workerDir, { recursive: true });
    let deadline = Date.now() + 240_000;
    while (Date.now() < deadline) {
      const entries = (await readdir(workerDir)).filter((n) => n.endsWith('.req.json')).sort();
      if (entries.length === 0) {
        await new Promise((r) => setTimeout(r, 150));
        continue;
      }
      for (const reqName of entries) {
        const reqPath = join(workerDir, reqName);
        let req;
        try { req = JSON.parse(await readFile(reqPath, 'utf8')); await unlink(reqPath); }
        catch { continue; }
        if (req.stop) { deadline = 0; break; }
        const jobScriptPath = join(workerDir, `${req.id}.ps1`);
        let jobFast = req.script;
        try {
          await stat(precompiledDll);
          jobFast = req.script.replace(/Add-Type @"\r?\nusing System;[\s\S]*?"@;/g, `Add-Type -Path '${precompiledDll}';`);
        } catch {}
        await writeFile(jobScriptPath, `${buildSetupProbeSessionScript(req.gui)}\r\n${jobFast}\r\n`, { encoding: 'utf8', mode: 0o600 });
        let jobRes;
        try {
          jobRes = { owner, stdout: await command('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', jobScriptPath], signal), wrapperExitCode: 0 };
        } catch (err) {
          jobRes = { owner, error: err.message, wrapperExitCode: 1 };
        }
        await persistLaunch(req.completionPath, jobRes);
        if (req.wrapperMarkerPath) await writeFile(req.wrapperMarkerPath, '0\r\n', { encoding: 'ascii', mode: 0o600 });
      }
    }
  } catch {}
} else if (process.argv.includes('--launch-gui')) {
  const specPath = process.argv[process.argv.indexOf('--launch-gui') + 1];
  launchNativeGuiAndRecordExit(JSON.parse(await readFile(specPath, 'utf8'))).catch((error) => {
    output({ error: error.message, cleanup: error.cleanup }); process.exitCode = 1;
  });
} else if (process.argv.includes('--rpc')) {
  createInterface({ input: process.stdin }).on('close', () => {
    for (const controller of jobs.values()) controller.abort(new Error('Native RPC input closed'));
    for (const entry of frontendProfiles.values()) entry.resource?.close().catch(error => {
      output({ event: 'receipt-error', error: error.message }); process.exitCode = 1;
    });
  }).on('line', (line) => {
    let request;
    try { request = JSON.parse(line); } catch (error) { output({ error: error.message }); return; }
    if (request.method === 'cancel') { jobs.get(request.args.id)?.abort(new Error('RPC cancelled')); return; }
    const controller = new AbortController(); jobs.set(request.id, controller);
    dispatch(request.method, request.args, controller.signal).then(
      (result) => output({ id: request.id, result }),
      (error) => output({ id: request.id, error: { code: error.code, message: error.message } }),
    ).finally(() => jobs.delete(request.id));
  });
}
