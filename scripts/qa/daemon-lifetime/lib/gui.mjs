// GUI driver per platform. Every launch is of a QA artifact the harness built and identity-checked;
// every quit, kill or termination targets only the recorded pid / QA bundle identifier.
//   mac:     direct exec of <QA.app>/Contents/MacOS/ferryx; quit by Apple Event to the QA bundle id;
//            forceTerminate through NSRunningApplication for the QA bundle id (osascript JXA).
//   linux:   systemd-run --user --scope --unit=<qa>-gui-<tag> (Q03 kills that scope); quit via MahoRD.
//   windows: launcher.ps1 started through MahoRD Win+R, each step gated on an observed receipt
//            (see launchWindows); Q03 TerminateJobObject; quit via MahoRD.

import { randomBytes } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { hjoin } from './hostpath.mjs';
import { assertBundleIdentifier, assertQaExecutable, qaBundleIdentifier, qaSupervisorName } from './guards.mjs';

export function macGuiExecutable(appPath) {
  return hjoin(appPath, 'Contents', 'MacOS', 'ferryx');
}

/** JXA that terminates only running applications with exactly this bundle identifier. */
export function macForceTerminateScript(bundleId) {
  return [
    "ObjC.import('AppKit');",
    `const apps = $.NSRunningApplication.runningApplicationsWithBundleIdentifier('${bundleId}');`,
    'const out = [];',
    'for (let i = 0; i < apps.count; i++) { const a = apps.objectAtIndex(i); out.push({ pid: a.processIdentifier, terminated: a.forceTerminate }); }',
    'JSON.stringify(out);',
  ].join('\n');
}

export function macQuitScript(bundleId) {
  return `tell application id "${bundleId}" to quit`;
}

export function linuxScopeUnit(runId, tag) {
  return `${qaSupervisorName(runId)}-gui-${tag}.scope`;
}

/** Win+R command line for the launcher; -NoBreakaway puts the GUI in a job that forbids breakaway (Q06w). */
export function windowsLauncherCommand({ root, exe, envFile, tag, noBreakaway = false }) {
  const flag = noBreakaway ? ' -NoBreakaway' : '';
  return `powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "${root}\\launcher.ps1" -Exe "${exe}" -RunDir "${root}" -EnvFile "${envFile}" -Tag ${tag}${flag}`;
}

/** Text MahoRD can type on maho-win: remote_type_text drops '-' and '_' there. */
export function mahordTypable(text) {
  return !/[-_]/.test(text);
}

/** The 8.3 short path of an existing file; throws when it still carries an untypable character. */
export async function windowsShortPath(agent, file) {
  const r = await agent.call('exec', { command: 'powershell.exe', args: ['-NoProfile', '-NonInteractive', '-Command',
    `(New-Object -ComObject Scripting.FileSystemObject).GetFile('${file.replace(/'/g, "''")}').ShortPath`] });
  const short = String(r.stdout ?? '').trim();
  if (r.status !== 0 || !short) throw new Error(`no 8.3 short path for ${file}: ${r.stderr}`);
  if (!mahordTypable(short)) throw new Error(`8.3 short path ${short} still has '-' or '_', which MahoRD cannot type`);
  return short;
}

/**
 * Bounded wait per Windows launch stage (ms). observerReady covers schtasks /Run plus PowerShell's
 * Add-Type compile in the interactive session; observerLifetime bounds the observer process itself.
 */
export const WINDOWS_LAUNCH_DEADLINES = Object.freeze({ observerReady: 60_000, observerArmed: 10_000, runDialog: 10_000, latched: 10_000, typed: 10_000, observerExit: 10_000, observerLifetime: 180_000, cmdStarted: 20_000, launcherStarted: 30_000, published: 60_000 });

/**
 * Files of one Windows launch. The receipts live in logs\, which teardown copies into the evidence
 * (runlog-<name>) before the run dir is removed. launcher-<tag>.started/.err are written by
 * fixtures/windows-gui-launcher.ps1 under the same names.
 */
export function windowsLaunchFiles(layout, tag) {
  const logs = layout.logs;
  return {
    envFile: `${layout.root}\\gui-${tag}.env.json`,
    // The file name carries no '-' or '_': both are legal 8.3 characters and would survive.
    launchCmd: `${layout.root}\\launch${String(tag).replace(/[^A-Za-z0-9]/g, '')}.cmd`,
    published: `${layout.root}\\gui-${tag}.json`,
    cmdStarted: `${logs}\\launch-${tag}.cmd.started`,
    launcherStarted: `${logs}\\launcher-${tag}.started`,
    launcherError: `${logs}\\launcher-${tag}.err`,
    receipt: `${logs}\\launch-${tag}.receipt.json`,
    screenshot: `${logs}\\launch-${tag}-failure.png.b64`,
    observerScript: `${layout.root}\\observer.ps1`,
    observerArgs: `${layout.root}\\observer-${tag}.args.json`,
    observerReady: `${logs}\\observer-${tag}.ready.json`,
    observerEvents: `${logs}\\observer-${tag}.events.jsonl`,
    observerError: `${logs}\\observer-${tag}.err`,
    observerArm: `${logs}\\observer-${tag}.arm`,
    observerTarget: (hwnd) => `${logs}\\observer-${tag}.target-${hwnd}`,
    observerStop: `${logs}\\observer-${tag}.stop`,
  };
}

/**
 * Per-run, per-launch scheduled task that runs fixtures/windows-focus-observer.ps1 in the
 * interactive session (the SSH agent runs in session 0 and cannot see its windows, measured in
 * Q00-run-dialog-capability-probe.md). The name carries the run id, so the schtasks guard allows it
 * and teardown's listQaJobs/remove find it even if launchWindows' own cleanup did not run.
 */
export function windowsObserverTask(runId, tag) {
  return `Ferryx\\${qaSupervisorName(runId)}-obs-${tag}`;
}

/** /TR of the observer task: conhost --headless keeps the console from opening a window. */
export function windowsObserverCommand(files, tag) {
  return `conhost.exe --headless powershell.exe -NoProfile -ExecutionPolicy Bypass -File ${files.observerScript} -Tag ${tag}`;
}

/**
 * The Run dialog as the observer reports it, by documented Win32 classes rather than localized
 * text or a dialog-specific control id: keyboard focus is in an "Edit" control (system class) whose
 * root window is a "#32770" dialog (system dialog class) owned by explorer.exe and in the
 * foreground. The explorer "cannot find" message box is also #32770 but focuses a Button; the
 * taskbar is Shell_TrayWnd. That this focus is the Run box's own edit is then confirmed, before
 * Enter, by the observer reading back exactly the typed text from that latched hwnd.
 */
export const RUN_DIALOG_FOCUS = Object.freeze({ processName: 'explorer.exe', rootClass: '#32770', focusedClass: 'Edit' });

export function isRunDialogFocus(record) {
  return Boolean(record) && record.kind === 'focus'
    && record.processName === RUN_DIALOG_FOCUS.processName
    && record.rootClass === RUN_DIALOG_FOCUS.rootClass
    && record.focusedClass === RUN_DIALOG_FOCUS.focusedClass
    && record.foreground === true
    && Number.isInteger(record.focusHwnd) && record.focusHwnd !== 0;
}

/**
 * waitLog pattern for the first observer record that isRunDialogFocus accepts. It follows the
 * observer's fixed field order (FocusObserver.Record) and is anchored on one line.
 */
export const RUN_DIALOG_FOCUS_PATTERN = '^\\{"kind":"focus","focusHwnd":[1-9]\\d*,"rootHwnd":-?\\d+,"processName":"explorer\\.exe","rootClass":"#32770","focusedClass":"Edit","parentClass":"[^"\\r\\n]*","foreground":true,"seq":\\d+\\}\\r?$';

/** waitLog pattern: the latched Edit read back the expected text, or focus left its dialog first. */
export const TYPED_OUTCOME_PATTERN = '^\\{"kind":"(text","match":true|lost)[^\\r\\n]*$';
/** Seconds a batch .cmd waits for its go file before giving up (ping -n 2 is about one second). */
export const GO_WAIT_SECONDS = 120;

/**
 * The .cmd the Run dialog executes: its first action is the "Run dialog executed me" receipt. With
 * goFile (gui.launchBatch), it then waits, bounded, for that file before starting the launcher, so
 * the typing of a batch is serialized while its GUIs still cold-start together.
 */
export function windowsLaunchCmdContent({ cmdStarted, command, goFile = null }) {
  const wait = goFile
    ? `set n=0\r\n:wait\r\nif exist "${goFile}" goto go\r\nset /a n+=1\r\nif %n% geq ${GO_WAIT_SECONDS} exit /b 1\r\nping -n 2 127.0.0.1 >nul\r\ngoto wait\r\n:go\r\n`
    : '';
  return `@echo off\r\necho started> "${cmdStarted}"\r\n${wait}start "" ${command}\r\n`;
}

/** The last focus/foreground/lost record of an observer events file, or null. */
export function lastObserverRecord(text) {
  const lines = String(text ?? '').split(/\r?\n/).filter((l) => l.trim());
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    try {
      const r = JSON.parse(lines[i]);
      if (['focus', 'foreground', 'lost'].includes(r.kind)) return r;
    } catch { /* a partial last line */ }
  }
  return null;
}

/** An explorer dialog (#32770: the Run box or its error message box) holds the foreground. */
export function isExplorerDialogForeground(record) {
  return Boolean(record) && record.processName === 'explorer.exe' && record.rootClass === '#32770' && record.foreground === true;
}

/** True when an observer events file holds exactly one record, "armed", and every line parses. */
export function onlyArmedRecord(text) {
  const lines = String(text ?? '').split(/\r?\n/).filter((l) => l.trim());
  try {
    const records = lines.map((l) => JSON.parse(l));
    return records.length === 1 && records[0].kind === 'armed';
  } catch {
    return false;
  }
}

/** Stages after Win+R whose leftovers (Run box, error box) are dismissed with Escape. */
export const DISMISS_STAGES = Object.freeze(['run-dialog-not-identified', 'observer-not-latched', 'typed-text-not-confirmed']);
/** Stages retried once, only when nothing executed and the dismissal was confirmed. */
export const RETRY_STAGES = Object.freeze(['run-dialog-not-identified', 'typed-text-not-confirmed']);

/** Windows "normal quit" request: the launcher sends WM_CLOSE (CloseMainWindow) to the GUI it started. */
export function windowsCloseFiles(layout, tag) {
  return { request: `${layout.root}\\gui-${tag}.close.request`, receipt: `${layout.root}\\gui-${tag}.closed.json` };
}

/** Request/answer of the launcher's main-window rect query (fixtures/windows-gui-launcher.ps1). */
export function windowsRectFiles(layout, tag) {
  return { request: `${layout.root}\\gui-${tag}.rect.request`, answer: `${layout.root}\\gui-${tag}.rect.json` };
}

/** Title bar point of a window rect: left part of the caption, clear of the caption buttons. */
export function titleBarPoint(rect) {
  return { x: rect.left + Math.min(200, Math.floor((rect.right - rect.left) / 4)), y: rect.top + 12 };
}

/** A Windows launch that stopped at a named stage; receipt is what was written to logs\. */
export class WindowsLaunchError extends Error {
  constructor(stage, detail, receipt) {
    super(`Windows GUI launch ${receipt.tag} failed at stage ${stage}: ${detail}`);
    this.name = 'WindowsLaunchError';
    this.stage = stage;
    this.receipt = receipt;
  }
}

/** The observer fixture, read from this checkout so the run uses exactly the reviewed source. */
const OBSERVER_SCRIPT = () => readFileSync(new URL('../fixtures/windows-focus-observer.ps1', import.meta.url), 'utf8');
/** The dialog-sweep fixture (preflight before the first Win+R of a run). */
const SWEEP_SCRIPT = () => readFileSync(new URL('../fixtures/windows-dialog-sweep.ps1', import.meta.url), 'utf8');

/**
 * Titles of the explorer dialogs a launch can leave behind, as measured on maho-win (ko-KR) with
 * the en-US equivalents: the Run box, and explorer's "location is not available" error box. The
 * "cannot find" box is titled with the typed path itself, so a title that is a path under this
 * run root's parent (long or 8.3 form) is ours too. Nothing else is ever selected.
 */
export const SWEEP_TITLES = Object.freeze({ run: Object.freeze(['실행', 'Run']), error: Object.freeze(['위치를 사용할 수 없습니다.', 'Location is not available']) });

/** Paths under which a path-titled explorer error box is a QA leftover: the run root's parent, long and 8.3. */
export function sweepPathPrefixes(runRootParent, shortParent = null) {
  return [runRootParent, shortParent].filter(Boolean).map((p) => `${String(p).replace(/\\+$/, '')}\\`.toLowerCase());
}

/**
 * Report-only list of the visible, titled non-QA windows the sweep saw (foreground first): what
 * could take focus from a QA launch (run-r6b Q05). Nothing here is closed or killed.
 */
export function foreignWindows(candidates) {
  const all = (candidates?.visibleWindows ?? []).filter((w) => !/^ferryx/i.test(String(w.processName ?? '')));
  return [...all.filter((w) => w.foreground), ...all.filter((w) => !w.foreground)]
    .map(({ hwnd, processName, className, title, foreground }) => ({ hwnd, processName, className, title, foreground: Boolean(foreground) }));
}

export function selectSweepTargets(windows, { pathPrefixes = [] } = {}) {
  const close = [];
  const kept = [];
  for (const w of windows ?? []) {
    const title = String(w.title ?? '');
    let why = null;
    if (String(w.processName ?? '').toLowerCase() !== 'explorer.exe') why = 'not explorer.exe';
    else if (w.className !== '#32770' || w.topLevel !== true) why = 'not a top-level #32770';
    else if (!Number.isInteger(w.hwnd) || w.hwnd === 0) why = 'no hwnd';
    const kind = SWEEP_TITLES.run.includes(title) ? 'run-dialog'
      : SWEEP_TITLES.error.includes(title) ? 'error-box'
        : pathPrefixes.some((p) => title.toLowerCase().startsWith(p)) ? 'error-box-path' : null;
    if (!why && !kind) why = 'title is neither the Run dialog nor an explorer error box';
    if (why) kept.push({ hwnd: w.hwnd ?? null, processName: w.processName ?? null, reason: why });
    else close.push({ hwnd: w.hwnd, kind, title, foreground: w.foreground === true });
  }
  return { close, kept };
}

export function createGuiDriver({ host, agent, runId, layout, allowedRoots, forbidden, mahord = null, guiEnv = {} }) {
  const platform = host.platform;
  const bundleId = qaBundleIdentifier(runId);
  let launches = 0;
  let batches = 0;
  let rectQueries = 0;
  // One interactive desktop, one Run dialog: the keyboard section of every Windows launch (taskbar
  // click .. cmd-started) runs alone. Two launches typing into one dialog concatenated their paths
  // (run-q00fix-all runlog-launch-g10-failure.png).
  let inputQueue = Promise.resolve();
  const withInput = (fn) => {
    const run = inputQueue.then(fn, fn);
    inputQueue = run.catch(() => {});
    return run;
  };
  /**
   * Preflight, once per run before the first Win+R: closes leftover explorer Run dialogs and error
   * boxes (run-r6: two dialogs from an earlier run kept the foreground and every launch failed).
   * A per-run task lists visible top-level #32770 windows in the interactive session; only those
   * selectSweepTargets accepts get WM_CLOSE, each on its exact hwnd after re-verification. The
   * receipt (logs\dialog-sweep.receipt.json) is the evidence; a failed sweep never throws, the
   * launch then fails on its own observed stage.
   */
  async function sweepExplorerDialogs() {
    const logs = layout.logs;
    const files = {
      script: `${layout.root}\\dialog-sweep.ps1`, candidates: `${logs}\\dialog-sweep.candidates.json`,
      close: `${logs}\\dialog-sweep.close.json`, result: `${logs}\\dialog-sweep.result.json`,
      error: `${logs}\\dialog-sweep.err`, receipt: `${logs}\\dialog-sweep.receipt.json`,
    };
    const task = `Ferryx\\${qaSupervisorName(runId)}-sweep`;
    const receipt = { task, candidates: null, selected: [], kept: [], result: null, error: null, cleanup: {} };
    try {
      await agent.call('writeFile', { file: files.script, content: SWEEP_SCRIPT() });
      const made = await agent.call('exec', { command: 'schtasks', args: ['/Create', '/TN', task, '/SC', 'ONLOGON', '/TR', `conhost.exe --headless powershell.exe -NoProfile -ExecutionPolicy Bypass -File ${files.script}`, '/F'] });
      if (made.status !== 0) throw new Error(`schtasks /Create ${task} exited ${made.status}: ${made.stderr || made.stdout}`);
      const ran = await agent.call('exec', { command: 'schtasks', args: ['/Run', '/TN', task] });
      if (ran.status !== 0) throw new Error(`schtasks /Run ${task} exited ${ran.status}: ${ran.stderr || ran.stdout}`);
      const listed = await Promise.race([
        agent.call('waitPath', { file: files.candidates, deadlineMs: 60_000 }).then((ok) => (ok ? 'listed' : 'timeout')),
        agent.call('waitPath', { file: files.error, deadlineMs: 60_000 }).then((ok) => (ok ? 'error' : new Promise(() => {}))),
      ]);
      if (listed === 'error') throw new Error(`sweep failed: ${await agent.call('readFile', { file: files.error })}`);
      if (listed !== 'listed') throw new Error(`no ${files.candidates} within 60000ms`);
      const candidates = receipt.candidates = JSON.parse(await agent.call('readFile', { file: files.candidates }));
      if (!Number.isInteger(candidates.sessionId) || candidates.sessionId === 0 || candidates.sessionId !== candidates.activeConsoleSessionId) {
        throw new Error(`sweep ran in session ${candidates.sessionId}, the active console session is ${candidates.activeConsoleSessionId}`);
      }
      const parent = layout.root.replace(/\\[^\\]+$/, '');
      const short = await windowsShortPath(agent, files.script).then((s) => s.replace(/\\[^\\]+\\[^\\]+$/, '')).catch(() => null);
      const { close, kept } = selectSweepTargets(candidates.windows, { pathPrefixes: sweepPathPrefixes(parent, short) });
      receipt.selected = close;
      receipt.kept = kept;
      receipt.foregroundStealers = foreignWindows(candidates);
      await agent.call('writeFile', { file: files.close, content: JSON.stringify({ hwnds: close.map((c) => c.hwnd) }) });
      if (!await agent.call('waitPath', { file: files.result, deadlineMs: 45_000 })) throw new Error(`no ${files.result} within 45000ms`);
      receipt.result = JSON.parse(await agent.call('readFile', { file: files.result }));
    } catch (error) {
      receipt.error = String(error?.message ?? error);
    } finally {
      for (const verb of ['/End', '/Delete']) {
        try {
          receipt.cleanup[verb] = (await agent.call('exec', { command: 'schtasks', args: verb === '/End' ? [verb, '/TN', task] : [verb, '/TN', task, '/F'] })).status;
        } catch (error) {
          receipt.cleanup[verb] = String(error?.message ?? error);
        }
      }
      try { await agent.call('writeFile', { file: files.receipt, content: JSON.stringify(receipt, null, 2) }); } catch (error) { receipt.receiptError = String(error?.message ?? error); }
    }
    return receipt;
  }

  async function verifiedBundle(appPath) {
    const r = await agent.call('exec', { command: '/usr/libexec/PlistBuddy', args: ['-c', 'Print :CFBundleIdentifier', hjoin(appPath, 'Contents', 'Info.plist')] });
    assertBundleIdentifier(r.stdout.trim(), runId);
  }

  function launchSpec(artifact, { env, tag = `g${++launches}`, noBreakaway = false } = {}) {
    const exe = artifact.guiExecutable;
    assertQaExecutable(exe, { allowedRoots, forbidden, platform });
    return { exe, fullEnv: { ...guiEnv, ...env }, tag, noBreakaway };
  }

  async function launch(artifact, options = {}) {
    const { exe, fullEnv, tag, noBreakaway } = launchSpec(artifact, options);
    if (platform === 'mac') {
      await verifiedBundle(artifact.appPath);
      const entry = await agent.call('spawn', { command: exe, args: [], env: fullEnv, role: `gui-${tag}`, executable: exe, logFile: hjoin(layout.logs, `gui-${tag}.log`) });
      return { tag, pid: entry.pid, executable: exe, startTime: entry.startTime };
    }
    if (platform === 'linux') {
      const unit = linuxScopeUnit(runId, tag);
      const spawned = await agent.call('spawn', {
        command: 'systemd-run', args: ['--user', '--scope', `--unit=${unit}`, '--', exe], env: fullEnv,
        role: `gui-${tag}`, executable: exe, logFile: hjoin(layout.logs, `gui-${tag}.log`), record: false,
      });
      // systemd-run --scope execs the GUI in place, so the spawned pid becomes the GUI.
      const ready = await agent.call('waitLog', { file: hjoin(layout.logs, `gui-${tag}.log`), pattern: 'Running scope as unit', deadlineMs: 30_000 });
      const entry = await agent.call('record', { pid: spawned.pid, role: `gui-${tag}`, executable: exe });
      return { tag, pid: entry.pid, executable: exe, startTime: entry.startTime, scope: unit, scopeReady: Boolean(ready.matched) };
    }
    return (await withInput(() => typeWindowsWithRetry({ exe, fullEnv, tag, noBreakaway }))).finish();
  }

  /**
   * Concurrent cold starts (Q06). items: [{ artifact, options }]; resolves like Promise.allSettled.
   * Windows types each launch alone (input mutex) into a .cmd that waits for one shared go file,
   * writes go once every typed launch has executed, then waits for all launchers together.
   */
  async function launchBatch(items) {
    if (platform !== 'windows') return Promise.allSettled(items.map(({ artifact, options }) => launch(artifact, options)));
    const goFile = `${layout.root}\\launchbatch${++batches}.go`;
    const typed = [];
    for (const { artifact, options } of items) {
      try {
        const spec = launchSpec(artifact, options);
        typed.push({ ok: true, value: await withInput(() => typeWindowsWithRetry({ ...spec, goFile })) });
      } catch (error) {
        typed.push({ ok: false, error });
      }
    }
    await agent.call('writeFile', { file: goFile, content: '1' });
    return Promise.allSettled(typed.map((t) => (t.ok ? t.value.finish() : Promise.reject(t.error))));
  }

  /**
   * One bounded retry of the keyboard section, only when it executed nothing and either the desktop
   * is clear or Win+R provably never arrived: the armed observer saw no focus or foreground change
   * at all, which no key on that desktop can produce. Then the MahoRD session is replaced first
   * (only3 Q13 g15/g16: the host session had ended, input went nowhere).
   */
  async function typeWindowsWithRetry(spec) {
    try {
      return await typeWindows({ ...spec, attempt: 1, prior: [] });
    } catch (error) {
      const r = error?.receipt;
      if (!(error instanceof WindowsLaunchError) || !RETRY_STAGES.includes(error.stage)) throw error;
      const f = windowsLaunchFiles(layout, spec.tag);
      const undelivered = error.stage === 'run-dialog-not-identified' && r?.dismissal?.onlyArmed === true;
      if (r?.dismissal?.clear !== true && !undelivered) throw error;
      if (await agent.call('exists', { file: f.cmdStarted })) throw error;
      let reconnected = null;
      if (undelivered) {
        if (typeof mahord.reconnect !== 'function') throw error;
        reconnected = await mahord.reconnect();
      }
      // A fresh observer reports from seq 1 again; its files must not carry the first attempt's records.
      for (const file of [f.observerReady, f.observerEvents, f.observerArm, f.observerStop, f.observerError]) await agent.call('remove', { target: file });
      return typeWindows({ ...spec, attempt: 2, prior: [{ failedStage: r.failedStage, stages: r.stages, dismissal: r.dismissal, screenshot: r.screenshot ?? null, undelivered, mahordReconnect: reconnected }] });
    }
  }

  /**
  /**
   * windows: the launcher is started on the interactive desktop through MahoRD Win+R, and every
   * step waits for an observed receipt with a bounded deadline instead of firing blind (r5 Q00:
   * Win+R, Ctrl+A, the text and Enter all returned within 8 ms and nothing ran). A read-only
   * observer (fixtures/windows-focus-observer.ps1, its own per-run task) reports focus records;
   * it never sends input, changes focus or launches anything.
   *   observer-ready     ready.json with this launch's nonce, an interactive session id, and the
   *                      pid whose start time the agent's OS probe confirms
   *   observer-armed     "armed" record, after the taskbar click and before Win+R
   *   run-dialog-focus   explorer #32770 dialog, Edit focused, foreground (isRunDialogFocus)
   *   latched            the observer latched that Edit's hwnd
   *   typed              the latched Edit read back exactly the typed short path; otherwise Enter
   *                      is never sent (a mangled path, H2, or a lost focus, H1, stops here)
   *   cmd-executed       logs\launch-<tag>.cmd.started, written by the .cmd itself
   *   launcher-started   logs\launcher-<tag>.started, the launcher's first statement
   *   published          gui-<tag>.json, unless logs\launcher-<tag>.err appears first
   * The observer task's cleanup (stop file, exit of the confirmed pid, schtasks /End and /Delete) is
   * registered right after /Create and runs on every path out of the observed section. A failed
   * stage writes logs\launch-<tag>.receipt.json and one MahoRD screenshot, then throws
   * WindowsLaunchError naming the stage.
   */
  async function typeWindows({ exe, fullEnv, tag, noBreakaway, goFile = null, attempt = 1, prior = [] }) {
    if (!mahord) throw new Error('Windows GUI launch needs MahoRD');
    const D = WINDOWS_LAUNCH_DEADLINES;
    const f = windowsLaunchFiles(layout, tag);
    const receipt = { tag, attempt, priorAttempts: prior, stages: [], failedStage: null, observer: null, dismissal: null };
    const mark = (stage, detail) => receipt.stages.push({ stage, ok: true, at: new Date().toISOString(), detail });
    const writeReceipt = () => agent.call('writeFile', { file: f.receipt, content: JSON.stringify(receipt, null, 2) });
    let armedObserver = false;
    // Escape only while the observer's newest record is a foreground explorer #32770, and only as
    // far as the observer confirms a following record each time; never a blind key.
    const dismiss = async () => {
      const d = { escapes: [], clear: false, last: null, onlyArmed: false };
      for (let i = 0; i < 3; i += 1) {
        const text = await agent.call('readFile', { file: f.observerEvents });
        const last = d.last = lastObserverRecord(text);
        if (!last) {
          // Positive evidence that Win+R reached nothing: every line parses and the armed record is
          // the only one. An unreadable file is not that evidence.
          if (i === 0 && armedObserver) d.onlyArmed = onlyArmedRecord(text);
          return d;
        }
        if (!isExplorerDialogForeground(last)) { d.clear = true; return d; }
        await mahord.key('Escape');
        const next = await agent.call('waitLog', { file: f.observerEvents, pattern: `,"seq":${last.seq + 1}\\}\\r?$`, flags: 'm', deadlineMs: 3_000 });
        d.escapes.push({ record: last, followed: Boolean(next.matched) });
        if (!next.matched) return d;
      }
      return d;
    };
    const fail = async (stage, detail) => {
      receipt.stages.push({ stage, ok: false, at: new Date().toISOString(), detail });
      receipt.failedStage = stage;
      const shot = attempt > 1 ? f.screenshot.replace('-failure', `-failure${attempt}`) : f.screenshot;
      try {
        const png = await mahord.screenshot();
        if (png) {
          await agent.call('writeFile', { file: shot, content: png.toString('base64') });
          receipt.screenshot = shot;
        } else {
          receipt.screenshotError = 'remote_take_screenshot returned no image';
        }
      } catch (error) {
        receipt.screenshotError = String(error?.message ?? error);
      }
      if (armedObserver && DISMISS_STAGES.includes(stage)) {
        try { receipt.dismissal = await dismiss(); } catch (error) { receipt.dismissal = { clear: false, error: String(error?.message ?? error) }; }
      }
      try { await writeReceipt(); } catch (error) { receipt.receiptError = String(error?.message ?? error); }
      throw new WindowsLaunchError(stage, detail, receipt);
    };
    const readRecord = (line) => JSON.parse(String(line).trim());

    await agent.call('writeFile', { file: f.envFile, content: JSON.stringify(fullEnv) });
    const command = windowsLauncherCommand({ root: layout.root, exe, envFile: f.envFile, tag, noBreakaway });
    // MahoRD remote_type_text drops '-' and '_' on maho-win (RunMRU proof, 2026-10-17), and the
    // launcher command is full of both. Write it into a .cmd in the run dir and type only that
    // file's 8.3 short path, which carries neither (runRoot is chosen so its 8.3 form has none).
    await agent.call('writeFile', { file: f.launchCmd, content: windowsLaunchCmdContent({ cmdStarted: f.cmdStarted, command, goFile }) });
    const short = await windowsShortPath(agent, f.launchCmd);

    const task = windowsObserverTask(runId, tag);
    const nonce = randomBytes(8).toString('hex');
    const observer = receipt.observer = { task, nonce, pid: null, sessionId: null, cleanup: null };
    await agent.call('writeFile', { file: f.observerScript, content: OBSERVER_SCRIPT() });
    await agent.call('writeFile', { file: f.observerArgs, content: JSON.stringify({ nonce, expectedText: short, deadlineMs: D.observerLifetime }) });

    let created = false;
    const cleanup = async () => {
      if (!created || observer.cleanup) return;
      const c = observer.cleanup = { stopRequested: false, exited: null, ended: null, deleted: null, errors: [] };
      const step = async (name, fn) => { try { await fn(); } catch (error) { c.errors.push({ step: name, error: String(error?.message ?? error) }); } };
      await step('stop', async () => { c.stopRequested = await agent.call('writeFile', { file: f.observerStop, content: nonce }); });
      if (observer.pid) {
        await step('exit', async () => {
          const r = await agent.call('waitExit', { pid: observer.pid, deadlineMs: D.observerExit });
          c.exited = !r?.timedOut;
        });
      }
      await step('end', async () => { c.ended = (await agent.call('exec', { command: 'schtasks', args: ['/End', '/TN', task] })).status; });
      await step('delete', async () => {
        const r = await agent.call('exec', { command: 'schtasks', args: ['/Delete', '/TN', task, '/F'] });
        c.deleted = r.status === 0;
        if (!c.deleted) c.errors.push({ step: 'delete', error: `schtasks /Delete exited ${r.status}: ${r.stderr || r.stdout}` });
      });
      try { await writeReceipt(); } catch (error) { c.errors.push({ step: 'receipt', error: String(error?.message ?? error) }); }
    };

    try {
      // Cleanup is armed before /Create returns: a /Create that half-succeeded is still deleted.
      created = true;
      // Same /Create form as jobs.mjs installLegacy: no /RU or /RP, so the task runs only while the
      // user is logged on, with the interactive token (as MahoRDHost does). ready.json's session id
      // is checked below, so a task that ran elsewhere fails at observer-wrong-session.
      const made = await agent.call('exec', { command: 'schtasks', args: ['/Create', '/TN', task, '/SC', 'ONLOGON', '/TR', windowsObserverCommand(f, tag), '/F'] });
      if (made.status !== 0) await fail('observer-task-not-created', `schtasks /Create ${task} exited ${made.status}: ${made.stderr || made.stdout}`);
      const ran = await agent.call('exec', { command: 'schtasks', args: ['/Run', '/TN', task] });
      if (ran.status !== 0) await fail('observer-task-not-run', `schtasks /Run ${task} exited ${ran.status}: ${ran.stderr || ran.stdout}`);
      const started = await Promise.race([
        agent.call('waitPath', { file: f.observerReady, deadlineMs: D.observerReady }).then((ok) => (ok ? 'ready' : 'timeout')),
        agent.call('waitPath', { file: f.observerError, deadlineMs: D.observerReady }).then((ok) => (ok ? 'error' : new Promise(() => {}))),
      ]);
      if (started === 'error') await fail('observer-error', `observer failed: ${await agent.call('readFile', { file: f.observerError })}`);
      if (started !== 'ready') await fail('observer-not-ready', `no ${f.observerReady} within ${D.observerReady}ms`);
      const ready = JSON.parse(await agent.call('readFile', { file: f.observerReady }));
      if (ready.nonce !== nonce) await fail('observer-not-owned', `ready.json nonce ${ready.nonce} is not this launch's`);
      // Only the confirmed pid is ever waited on; ownership is nonce + OS start time, never a name.
      const os = await agent.call('probe', { pid: ready.pid });
      if (!os || String(os.startTime) !== String(ready.start) || !/\\powershell\.exe$/i.test(String(os.executable))) {
        await fail('observer-not-owned', `pid ${ready.pid} identity ${JSON.stringify(os)} does not match ready.json start ${ready.start}`);
      }
      observer.pid = ready.pid;
      observer.sessionId = ready.sessionId;
      if (!Number.isInteger(ready.sessionId) || ready.sessionId === 0 || ready.sessionId !== ready.activeConsoleSessionId) {
        await fail('observer-wrong-session', `observer runs in session ${ready.sessionId}, the active console session is ${ready.activeConsoleSessionId}`);
      }
      if (ready.hooks !== 3) await fail('observer-not-ready', `observer registered ${ready.hooks} WinEvent hooks, expected 3`);
      mark('observer-ready', { task, pid: ready.pid, sessionId: ready.sessionId });

      await mahord.click(3000, 1580); // blank taskbar: focus the shell so Win+R reaches it
      // Armed after the click, so only focus changes from Win+R onwards are reported.
      await agent.call('writeFile', { file: f.observerArm, content: nonce });
      const armed = await agent.call('waitLog', { file: f.observerEvents, pattern: '^\\{"kind":"armed",', flags: 'm', deadlineMs: D.observerArmed });
      if (!armed.matched) await fail('observer-not-armed', `no armed record within ${D.observerArmed}ms`);
      armedObserver = true;
      mark('observer-armed', {});

      await mahord.hotkey(['Meta', 'r']);
      const focus = await agent.call('waitLog', { file: f.observerEvents, pattern: RUN_DIALOG_FOCUS_PATTERN, flags: 'm', deadlineMs: D.runDialog });
      const record = focus.matched ? readRecord(focus.matched) : null;
      if (!isRunDialogFocus(record)) await fail('run-dialog-not-identified', `no foreground explorer #32770 dialog with an Edit focused within ${D.runDialog}ms after Win+R (records in ${f.observerEvents})`);
      mark('run-dialog-focus', record);

      await agent.call('writeFile', { file: f.observerTarget(record.focusHwnd), content: nonce });
      const latched = await agent.call('waitLog', { file: f.observerEvents, pattern: `^\\{"kind":"latched","focusHwnd":${record.focusHwnd},`, flags: 'm', deadlineMs: D.latched });
      if (!latched.matched) await fail('observer-not-latched', `the observer did not latch hwnd ${record.focusHwnd} within ${D.latched}ms`);
      mark('latched', { focusHwnd: record.focusHwnd });

      // The Run dialog pre-fills its last entry and typed text is appended to it: select it first.
      await mahord.hotkey(['Control', 'a']);
      await mahord.type(short);
      const typed = await agent.call('waitLog', { file: f.observerEvents, pattern: TYPED_OUTCOME_PATTERN, flags: 'm', deadlineMs: D.typed });
      const outcome = typed.matched ? readRecord(typed.matched) : null;
      if (outcome?.kind !== 'text' || outcome.match !== true) {
        await fail('typed-text-not-confirmed', outcome?.kind === 'lost'
          ? `focus left the Run dialog before the text was confirmed: ${JSON.stringify(outcome)}`
          : `the latched Edit did not read back ${short} within ${D.typed}ms; Enter was not sent`);
      }
      mark('typed', { text: short, length: outcome.length });
      await mahord.key('Enter');
    } finally {
      await cleanup();
    }
    mark('observer-stopped', observer.cleanup);
    if (!await agent.call('waitPath', { file: f.cmdStarted, deadlineMs: D.cmdStarted })) {
      await fail('cmd-not-executed', `the Run dialog did not execute ${short}: no ${f.cmdStarted} within ${D.cmdStarted}ms`);
    }
    mark('cmd-executed', { file: f.cmdStarted });
    return { finish: () => finishWindows({ exe, tag, f, receipt, mark, fail, writeReceipt, goFile }) };
  }

  /** After cmd-started (outside the input mutex): launcher started, GUI published and recorded. */
  async function finishWindows({ exe, tag, f, receipt, mark, fail, writeReceipt, goFile }) {
    const D = WINDOWS_LAUNCH_DEADLINES;
    // A batch launcher starts only after go; its wait counts from there, not from typing.
    if (goFile) mark('go', { file: goFile });
    if (!await agent.call('waitPath', { file: f.launcherStarted, deadlineMs: D.launcherStarted })) {
      await fail('launcher-not-started', `${f.launchCmd} ran but launcher.ps1 wrote no ${f.launcherStarted} within ${D.launcherStarted}ms`);
    }
    mark('launcher-started', { file: f.launcherStarted });
    // Whichever comes first: the published gui json, or the launcher's error transcript.
    const outcome = await Promise.race([
      agent.call('waitPath', { file: f.published, deadlineMs: D.published }).then((ok) => (ok ? 'published' : 'timeout')),
      agent.call('waitPath', { file: f.launcherError, deadlineMs: D.published }).then((ok) => (ok ? 'error' : new Promise(() => {}))),
    ]);
    if (outcome === 'error') {
      await fail('launcher-error', `launcher.ps1 failed: ${await agent.call('readFile', { file: f.launcherError })}`);
    }
    if (outcome !== 'published') await fail('launcher-not-published', `launcher.ps1 started but did not publish ${f.published} within ${D.published}ms`);
    const info = JSON.parse(await agent.call('readFile', { file: f.published }));
    mark('published', { pid: info.pid, launcherPid: info.launcherPid });
    const entry = await agent.call('record', { pid: info.pid, role: `gui-${tag}`, executable: exe });
    await writeReceipt();
    return { tag, pid: entry.pid, executable: exe, startTime: entry.startTime, jobAssigned: info.jobAssigned, noBreakaway: info.noBreakaway === true };
  }

  /**
   * Windows: make the recorded GUI's main window the foreground before MahoRD keys go to it. A GUI
   * started from the Run dialog's .cmd can open behind the console that started it (run-q11
   * only-q11-q13-r2: Escape went to that console, the setup wizard stayed open). The launcher
   * reports the window rect and the foreground hwnd; if they differ, its own title bar is clicked
   * and the query repeated. Other platforms need no focus step.
   */
  async function focus(gui) {
    if (platform !== 'windows') return { skipped: platform };
    if (!mahord) throw new Error('Windows GUI focus needs MahoRD');
    const f = windowsRectFiles(layout, gui.tag);
    const query = async () => {
      const seq = ++rectQueries;
      await agent.call('writeFile', { file: f.request, content: String(seq) });
      const seen = await agent.call('waitLog', { file: f.answer, pattern: `"seq":${seq},`, deadlineMs: 10_000 });
      if (!seen.matched) throw new Error(`launcher ${gui.tag} answered no rect query ${seq} within 10000ms`);
      return JSON.parse(await agent.call('readFile', { file: f.answer }));
    };
    const before = await query();
    if (!before.ok || !before.hwnd) return { focused: false, before };
    if (before.foreground === before.hwnd) return { focused: true, clicked: null, before };
    const point = titleBarPoint(before);
    await mahord.click(point.x, point.y);
    const after = await query();
    return { focused: after.foreground === after.hwnd, clicked: point, before, after };
  }

  /**
   * Normal quit; resolves with the exit event of the recorded pid ({ skipped } without a GUI).
   * Windows asks the launcher that started this GUI to send WM_CLOSE to its main window
   * (CloseMainWindow, the path of a user's close button) instead of a global Alt+F4, which closes
   * whatever window holds the foreground (Q01 run-q00fix-all: the GUI never exited).
   */
  async function quit(gui) {
    if (!gui?.pid) return { skipped: 'no GUI' };
    const exited = agent.call('waitExit', { pid: gui.pid, deadlineMs: 30_000 });
    if (platform === 'mac') {
      await agent.call('exec', { command: '/usr/bin/osascript', args: ['-e', macQuitScript(bundleId)] });
    } else if (platform === 'windows') {
      await agent.call('writeFile', { file: windowsCloseFiles(layout, gui.tag).request, content: '1' });
    } else {
      if (!mahord) throw new Error('remote GUI quit needs MahoRD');
      // MahoRD remote_key_press takes one key; chords go through remote_hotkey. omarchy's Hyprland
      // binds SUPER+Q to close window
      // (~/.config/hypr/bindings.lua:52) and Ferryx has no Ctrl+Q quit on Linux (shortcuts.ts:453 is macOS only).
      await mahord.hotkey(['Meta', 'q']);
    }
    const out = await exited;
    if (platform !== 'windows') return out;
    return { ...out, close: await agent.call('readFile', { file: windowsCloseFiles(layout, gui.tag).receipt }) };
  }

  /** Q02: SIGKILL / Stop-Process of the recorded GUI pid only. */
  async function kill(gui) {
    if (!gui?.pid) return { skipped: 'no GUI' };
    const exited = agent.call('waitExit', { pid: gui.pid, deadlineMs: 30_000 });
    await agent.call('kill', { pid: gui.pid, signal: 'SIGKILL' });
    return exited;
  }

  /** Q03: OS-attributed termination. */
  async function forceTerminate(gui) {
    if (!gui?.pid) return { skipped: 'no GUI' };
    const exited = agent.call('waitExit', { pid: gui.pid, deadlineMs: 30_000 });
    let detail;
    if (platform === 'mac') {
      const check = await agent.call('check', { pid: gui.pid });
      if (check !== 'match') throw new Error(`GUI pid ${gui.pid} identity is ${check}`);
      detail = await agent.call('exec', { command: '/usr/bin/osascript', args: ['-l', 'JavaScript', '-e', macForceTerminateScript(bundleId)] });
    } else if (platform === 'linux') {
      detail = await agent.call('exec', { command: 'systemctl', args: ['--user', 'kill', '--signal=SIGKILL', gui.scope] });
    } else {
      await agent.call('writeFile', { file: `${layout.root}\\gui-${gui.tag}.terminate.request`, content: '1' });
      detail = { requested: true };
    }
    return { exited: await exited, detail };
  }

  return { launch, launchBatch, focus, quit, kill, forceTerminate, sweepExplorerDialogs, bundleId };
}
