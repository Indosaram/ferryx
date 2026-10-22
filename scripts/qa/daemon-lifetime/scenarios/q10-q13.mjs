// Q10-Q13: autostart opt-out, headless use, SSH continuity without replay, live-entry honesty,
// and legacy single-job migration.

import path from 'node:path';
import { hjoin } from '../lib/hostpath.mjs';
import { assertion, block } from '../lib/summary.mjs';
import { data as D, isLive, liveSessions } from '../lib/wire.mjs';
import {
  evidence, dataConn, controlConn, ownerIdentity, nonceRoundTrip, assertSameOwner, retireOwner, freshOwner, nonce,
  echoCommand, interrupt, setAsideHeadCatalog, requireOwner, SHELL_READY_TIMEOUT_MS, DAEMON_READY_TIMEOUT_MS,
  enterKey,
} from './common.mjs';
import { startSshd, sshRestoreSession, recordSshHelper } from '../lib/sshd.mjs';
import { typeVerified, renderEcho, renderConptyInput } from '../lib/typeverified.mjs';
import { legacyLogFiles } from '../lib/jobs.mjs';

export const OPTOUT_FILE = 'daemon-supervisor.optout'; // supervisor.rs OPTOUT_FILE_NAME, in <FERRYX_DATA_DIR>/remote

async function loginRegistration(ctx, key) {
  if (ctx.platform === 'mac') {
    const plist = (await ctx.agent.call('readFile', { file: hjoin(ctx.layout.launchAgents, `${ctx.jobs.label(key)}.plist`) })) ?? '';
    return { runAtLoad: /<key>RunAtLoad<\/key>\s*<true\/>/.test(plist), raw: plist };
  }
  if (ctx.platform === 'linux') {
    const job = await ctx.jobs.inspect(key);
    return { enabled: job.unitFileState === 'enabled', raw: job.unitFileState };
  }
  const xml = await ctx.agent.call('exec', { command: 'schtasks', args: ['/Query', '/TN', ctx.jobs.task(key), '/XML'] });
  return { logonTrigger: /<LogonTrigger/.test(xml.stdout), raw: xml.stdout };
}

const autostarts = (reg) => reg.runAtLoad ?? reg.enabled ?? reg.logonTrigger;

/**
 * Keyboard path to the General switch "Restart background service automatically"
 * (ui/src/components/settings/GeneralSection.tsx:447, id general-daemon-supervisor) on a fresh QA
 * profile: settings.toggle is mod+, (ui/src/lib/shortcuts.ts:369; mod is Control off macOS) and
 * opens on General with focus on "Back to app" (SettingsDialog.tsx:117). Tab then walks the nine
 * section buttons (no Plan entry: a fresh QA profile has no account token, SettingsDialog.tsx:95)
 * and the three General controls before the switch. Space toggles the focused Radix switch.
 */
export const SUPERVISOR_SWITCH_TABS = 1 + 9 + 3;

/** GUI log line of cmd_daemon_supervisor_get (ipc/agent_integration.rs), which General calls on mount. */
export const SETTINGS_OPENED_RECEIPT = 'Daemon supervisor status read';
export const SETTINGS_OPEN_ATTEMPTS = 3;
export const SETTINGS_OPEN_DEADLINE_MS = 10_000;

/** A pattern that matches once the log holds at least `count` receipt lines. */
export function nthReceiptPattern(receipt, count) {
  return `(?:${receipt}[\\s\\S]*?){${count}}`;
}

/**
 * Opens Settings with its shortcut and proves it opened: each press must add one
 * SETTINGS_OPENED_RECEIPT line to the GUI log (General mounts and reads the supervisor status).
 * run-9a5db442 pressed the shortcut into a webview that had not rendered yet and toggled blind.
 * A press with no new receipt within the deadline did not open the dialog, so pressing again is
 * safe; a receipt that arrives late only means a later press closes the dialog, which the next
 * press then reopens with its own receipt. Bounded by SETTINGS_OPEN_ATTEMPTS.
 */
export async function openSettings(ctx, logFile) {
  const attempts = [];
  const count = async () => (((await ctx.agent.call('readFile', { file: logFile })) ?? '').split(SETTINGS_OPENED_RECEIPT).length - 1);
  for (let i = 0; i < SETTINGS_OPEN_ATTEMPTS; i += 1) {
    const before = await count();
    await ctx.mahord.hotkey(['Control', ',']);
    const seen = await ctx.agent.call('waitLog', { file: logFile, pattern: nthReceiptPattern(SETTINGS_OPENED_RECEIPT, before + 1), deadlineMs: SETTINGS_OPEN_DEADLINE_MS });
    attempts.push({ before, opened: Boolean(seen.matched) });
    if (seen.matched) return { opened: true, attempts };
  }
  return { opened: false, attempts };
}

/**
 * Drives the real settings toggle through MahoRD; the product writes the marker as its receipt.
 * Nothing is toggled unless Settings provably opened.
 */
async function toggleAutostartOffThroughSettings(ctx, optout, logFile) {
  if (!ctx.mahord) throw new Error('the settings toggle needs MahoRD on remote hosts');
  const opened = await openSettings(ctx, logFile);
  if (!opened.opened) {
    return { opened, marker: false, before: null, after: await ctx.mahord.screenshot().catch(() => null) };
  }
  // The receipt is logged when cmd_daemon_supervisor_get answers, which is what renders the switch,
  // so focus is walked only after the switch exists.
  for (let i = 0; i < SUPERVISOR_SWITCH_TABS; i += 1) await ctx.mahord.key('Tab');
  const before = await ctx.mahord.screenshot().catch(() => null);
  await ctx.mahord.key('Space');
  const marker = await ctx.agent.call('waitPath', { file: optout, deadlineMs: 30_000 });
  const after = await ctx.mahord.screenshot().catch(() => null);
  await ctx.mahord.key('Escape');
  return { opened, marker, before, after };
}

/** remove_autostart runs right after the marker write in the same command: wait for its effect. */
async function waitAutostartRemoved(ctx, key, deadlineMs = 30_000) {
  const end = Date.now() + deadlineMs;
  let reg = await loginRegistration(ctx, key);
  while (autostarts(reg) !== false && Date.now() < end) {
    await new Promise((resolve) => setTimeout(resolve, 500));
    reg = await loginRegistration(ctx, key);
  }
  return reg;
}

export async function Q10(ctx, result) {
  if (!requireOwner(ctx, result)) return;
  const key = ctx.keys.A;
  const owner = await ownerIdentity(ctx);
  const optout = hjoin(ctx.layout.data, 'remote', OPTOUT_FILE);
  // Toggle through the product: Settings > General > "Restart background service automatically"
  // calls cmd_daemon_supervisor_set(false) (ipc/agent_integration.rs:100-102), which writes the
  // marker and then calls remove_autostart. Writing the marker directly only skips future
  // registration (supervisor.rs:256-259) and never removes an existing one.
  // A fresh GUI holds keyboard focus, so the shortcut reaches its window.
  await ctx.gui.quit(ctx.state.gui);
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts.A);
  const guiLog = hjoin(ctx.layout.logs, `gui-${ctx.state.gui.tag}.log`);
  const settled = await ctx.agent.call('waitLog', { file: guiLog, pattern: 'Daemon supervisor armed', deadlineMs: DAEMON_READY_TIMEOUT_MS });
  // Read after this A GUI armed: its setup reconciles login registration (owner_admission.rs
  // ensure_if_absent_with), which is what registers A. Before that, the last GUI of another build
  // (Q08b's v1bump) may legitimately hold the registration.
  const regBefore = await loginRegistration(ctx, key);
  assertion(result, 'newest generation registered for login before opt-out', autostarts(regBefore) === true, regBefore.raw?.slice?.(0, 400) ?? regBefore.raw);
  // The owner's own tree right before the toggle: Q00's watchdog was replaced long ago (run-int2-all Q10).
  const treeBefore = ctx.platform === 'windows' ? await ctx.jobs.windowsTree(ctx.stagedRoot, { ownerPid: owner.pid }) : null;
  const toggle = await toggleAutostartOffThroughSettings(ctx, optout, guiLog);
  if (toggle.before) await evidence(ctx, result, 'settings-before-space.png.b64', toggle.before.toString('base64'));
  if (toggle.after) await evidence(ctx, result, 'settings-after-space.png.b64', toggle.after.toString('base64'));
  assertion(result, 'Settings opened (cmd_daemon_supervisor_get receipt) before the toggle', toggle.opened.opened, toggle.opened.attempts);
  assertion(result, 'settings toggle wrote the opt-out marker (product receipt)', toggle.marker === true, optout);
  const regAfter = toggle.marker ? await waitAutostartRemoved(ctx, key) : await loginRegistration(ctx, key);
  const ownerAfter = await ownerIdentity(ctx);
  await evidence(ctx, result, 'toggle.json', { regBefore, regAfter, settled, opened: toggle.opened, marker: toggle.marker, owner, ownerAfter });
  assertion(result, 'opt-out removed login autostart', autostarts(regAfter) === false, regAfter.raw?.slice?.(0, 400) ?? regAfter.raw);
  assertSameOwner(result, owner, ownerAfter, 'opt-out toggle never stops the running owner');
  if (ctx.platform === 'windows') {
    const tree = await ctx.jobs.windowsTree(ctx.stagedRoot, { ownerPid: owner.pid });
    assertion(result, 'task re-import kept watchdog and daemon pids (carry-over todo 10 N4)', Boolean(treeBefore?.watchdog) && tree.watchdog?.pid === treeBefore.watchdog.pid && tree.daemon?.pid === owner.pid, { before: treeBefore, after: tree });
    ctx.carry.push({ item: 'todo 10 N4: /Create /XML /F re-import keeps watchdog and daemon alive', evidence: path.posix.join(ctx.evidenceDir, 'Q10-toggle.json') });
  }
  // Opted-out launch with an Absent owner: the newest generation still starts through its OS job.
  const retired = await retireOwner(ctx);
  assertion(result, 'owner retired before the opted-out launch', retired.retired, retired);
  if (ctx.platform === 'mac') {
    // Make the generation job NotRegistered so the opted-out bootstrap + plain kickstart path runs.
    await ctx.agent.call('exec', { command: 'launchctl', args: ['bootout', `gui/${ctx.uid}/${ctx.jobs.label(key)}`] });
    await ctx.agent.call('remove', { target: hjoin(ctx.layout.launchAgents, `${ctx.jobs.label(key)}.plist`) });
  }
  const fresh = await freshOwner(ctx, ctx.artifacts.A);
  const job = await ctx.jobs.inspect(key);
  const guiText = (await ctx.agent.call('readFile', { file: hjoin(ctx.layout.logs, `gui-${ctx.state.gui.tag}.log`) })) ?? '';
  await evidence(ctx, result, 'optout-launch.json', { fresh, job });
  await evidence(ctx, result, 'optout-launch-gui.log', guiText);
  const jobPid = ctx.platform === 'windows' ? (await ctx.jobs.windowsTree(ctx.stagedRoot)).daemon?.pid : job.pid;
  assertion(result, 'opted-out launch started the owner under the OS job, not as a GUI child', jobPid === fresh.pid, { jobPid, owner: fresh.pid });
  const row = (await ctx.agent.call('table')).find((r) => r.pid === fresh.pid);
  assertion(result, 'owner parent is not the GUI', row && row.ppid !== ctx.state.gui.pid, row);
  if (ctx.platform === 'mac') {
    assertion(result, 'opted-out NotRegistered job: plist has RunAtLoad and KeepAlive false, bootstrap + plain kickstart started it (carry-over todo 10 N2)', /<key>RunAtLoad<\/key>\s*<false\/>/.test((await loginRegistration(ctx, key)).raw) && job.state === 'Running', job.state);
    ctx.carry.push({ item: 'todo 10 N2 / DoneClaim risk: bootstrap does not start a RunAtLoad=false KeepAlive=false plist; plain kickstart does', evidence: path.posix.join(ctx.evidenceDir, 'Q10-optout-launch.json') });
  }
  await ctx.agent.call('remove', { target: optout });
}

export async function Q10b(ctx, result) {
  const retired = await retireOwner(ctx);
  assertion(result, 'owner retired', retired.retired, retired);
  const rows = await ctx.agent.call('table');
  const guis = rows.filter((r) => r.executable && ctx.guiExecutables.some((g) => g.toLowerCase() === r.executable.toLowerCase()));
  assertion(result, 'no QA GUI process running (recorded-path check of every process)', guis.length === 0, guis);
  const key = ctx.keys.A;
  const start = await ctx.jobs.start(key);
  await evidence(ctx, result, 'job-start.json', start);
  const changed = await ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid: retired.pid, deadlineMs: DAEMON_READY_TIMEOUT_MS });
  const ctl = changed.owner ? await controlConn(ctx) : null;
  const hello = ctl ? await ctl.hello() : null;
  const job = await ctx.jobs.inspect(key);
  const jobPid = ctx.platform === 'windows' ? (await ctx.jobs.windowsTree(ctx.stagedRoot)).daemon?.pid : job.pid;
  assertion(result, 'control Hello answers within DAEMON_READY_TIMEOUT', hello?.ok === true, hello);
  assertion(result, 'Hello pid equals the QA job pid', hello?.pid === jobPid, { hello: hello?.pid, jobPid });
  // No Hello means no headless owner to use: fail cleanly with what was observed (run-r6b threw a
  // TypeError here on hello.pid) and let the scenario finally/recovery handle teardown.
  if (!hello?.pid) return block(result, 'OWNER_UNAVAILABLE', { stage: 'Q10b headless Hello', ownerChange: changed, hello, jobPid });
  await ctx.agent.call('record', { pid: hello.pid, role: 'daemon-headless', executable: (await ctx.agent.call('probe', { pid: hello.pid })).executable });
  // The QA build's own CLI client entry point: `ferryx agents list` (agent_control) against the QA runtime.
  const cliEnv = ctx.isolationEnv;
  const list = await ctx.agent.call('exec', { command: ctx.artifacts.A.cliExecutable, args: ['agents', 'list', '--json'], env: cliEnv, deadlineMs: 90_000 });
  await evidence(ctx, result, 'cli-list.log', `exit ${list.status}\n${list.stdout}\n${list.stderr}`);
  assertion(result, 'CLI client listed sessions through the daemon', list.status === 0, list.status);
  const conn = await dataConn(ctx);
  ctx.state.conn = conn;
  // Plan deviation (recorded in the DoneClaim): src-tauri/src/bin/cli.rs has no spawn-capable
  // subcommand (`agents` offers list/read/send/answer/stop only, agent_control/mod.rs
  // parse_agents_cli), and todo 12 adds no CLI command. The local session is spawned through the
  // QA build's own daemon client protocol (data-protocol Spawn) from the harness instead.
  await evidence(ctx, result, 'spawn-path.json', { deviation: 'no spawn subcommand in src-tauri/src/bin/cli.rs; spawned over the QA daemon data protocol', cli: ctx.artifacts.A.cliExecutable });
  const spawned = await conn.call(D.spawnLocal({ clientRequestId: `qa-headless-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir }));
  if (spawned.type === 'error' && !ctx.workspaceRegistered) await conn.call(D.registerWorkspace(ctx.workspaceId, ctx.repoDir));
  const sid = spawned.sessionId ?? (await conn.call(D.spawnLocal({ clientRequestId: `qa-headless-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir }))).sessionId;
  assertion(result, 'one local session spawned headless', Boolean(sid), spawned);
  ctx.state.localA = sid;
  const list2 = await ctx.agent.call('exec', { command: ctx.artifacts.A.cliExecutable, args: ['agents', 'list', '--json'], env: cliEnv, deadlineMs: 90_000 });
  await evidence(ctx, result, 'cli-list-after.log', `exit ${list2.status}\n${list2.stdout}\n${list2.stderr}`);
  const after = await ctl.hello();
  assertion(result, 'Hello pid unchanged after the CLI exits', after?.pid === hello.pid, { before: hello.pid, after: after?.pid });
  await ctl.close();
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts.A);
}

/** native_receipts_dir: <barrier dir parent>/receipts (qa_barrier.rs native_receipts_dir). */
export function nativeEventsFile(layout) {
  return hjoin(path.posix.dirname(layout.barriers.replace(/\\/g, '/')), 'receipts', 'native-events.jsonl');
}

/** One native-events line of kind "attached" whose attach tuple names this backend session. */
export function attachedPattern(sessionId) {
  return `^(?=[^\\n]*"kind":"attached")(?=[^\\n]*"backendSessionId":"${sessionId}")[^\\n]*$`;
}

/**
 * Wait for the restored tab's attach before and after one Escape. A fresh QA profile can open the
 * first-run setup wizard (App.tsx shouldAutoOpenOnboarding): a role="dialog" that, off macOS,
 * hides native surfaces (nativeTerminalVisibility.tsx), so no tab attaches while it is open. On
 * Windows the notifications permission reports Unknown with an open-settings affordance, which
 * keeps the permissions step pending (onboarding.ts visiblePermissionKeys), so it opens there
 * (run-q11 Q11-attach-screenshot.png); on Linux that step is hidden. Escape is the wizard's
 * "remind later" (WelcomeWizard.tsx handleKeyDown). It is sent only when no attach came, so no
 * native terminal owns the key; one that reached the SSH session would fail the exact-log check.
 */
export const ATTACH_BEFORE_ESCAPE_MS = 20_000;
export const ATTACH_AFTER_ESCAPE_MS = 40_000;
export async function waitRestoredAttach(ctx, sid, gui) {
  const wait = (deadlineMs) => ctx.agent.call('waitLog', { file: nativeEventsFile(ctx.layout), pattern: attachedPattern(sid), flags: 'm', deadlineMs });
  const first = await wait(ATTACH_BEFORE_ESCAPE_MS);
  if (first.matched) return { ...first, escapeSent: false, focus: null };
  // The key must reach the GUI, not the window that holds the foreground (Windows: see gui.focus).
  const focus = await ctx.gui.focus(gui);
  await ctx.mahord.key('Escape');
  return { ...(await wait(ATTACH_AFTER_ESCAPE_MS)), escapeSent: true, focus };
}

/**
 * Windows: close the first-run setup wizard before any key is typed. On a fresh QA profile it
 * always opens there (the notifications permission is Unknown with an open-settings affordance,
 * onboarding.ts visiblePermissionKeys; nothing in the run persists a dismissal: Escape is
 * "remind later", App.tsx onRemindLater), but it opens from a timer 1.2 s after mount
 * (App.tsx shouldAutoOpenOnboarding effect), so it can land after the restored tab attached and
 * cover the pane mid-typing (run-q11 all/Q11-switch-debug.jsonl: "which=dialog:Welcome to Ferryx"
 * at 22:32:01.345, after the 13th key; the 14th was never echoed). The app has no flag a QA
 * profile could set beforehand (the state is WebView localStorage), so the harness waits for the
 * wizard's own event: the pane records terminal.surface.skipped naming the dialog
 * (NativeTerminalPane.tsx) in the GUI's %TEMP%\ferryx-switch-debug.jsonl, and Q11 points TEMP at
 * this run's own dir so no other run's lines can match. Escape goes to the GUI only after
 * gui.focus; the pane's next visible gate.state proves the wizard closed. The wizard opens once per
 * mount, so nothing can cover the pane after that. Elsewhere this is a no-op.
 */
export const WIZARD_OPEN_MS = 60_000;
export const WIZARD_CLOSED_MS = 10_000;
export const WIZARD_ESCAPE_SENDS = 3;
export function expectsSetupWizard(platform) {
  return platform === 'windows';
}
export function guiTempDir(layout) {
  return hjoin(layout.root, 'gui-tmp');
}
export function wizardOpenPattern(pane) {
  return `"terminal\\.surface\\.skipped"[^\\n]*"localSessionId":"${pane}"[^\\n]*which=dialog:Welcome to Ferryx`;
}
export function wizardClosedPattern(pane) {
  return `which=dialog:Welcome to Ferryx[\\s\\S]*"terminal\\.surface\\.input\\.gate\\.state"[^\\n]*"paneIdentity":"${pane}"[^\\n]*"visible":true`;
}
export async function dismissSetupWizard(ctx, gui, pane) {
  if (!expectsSetupWizard(ctx.platform)) return { expected: false };
  const file = hjoin(guiTempDir(ctx.layout), 'ferryx-switch-debug.jsonl');
  const opened = await ctx.agent.call('waitLog', { file, pattern: wizardOpenPattern(pane), deadlineMs: WIZARD_OPEN_MS });
  if (!opened.matched) return { expected: true, opened: false, closed: false, escapes: 0 };
  let closed = { matched: null };
  let escapes = 0;
  const focus = [];
  // A lost Escape (MahoRD drops keys) leaves the pane covered, so it is resent (bounded).
  while (!closed.matched && escapes < WIZARD_ESCAPE_SENDS) {
    focus.push(await ctx.gui.focus(gui));
    await ctx.mahord.key('Escape');
    escapes += 1;
    closed = await ctx.agent.call('waitLog', { file, pattern: wizardClosedPattern(pane), deadlineMs: WIZARD_CLOSED_MS });
  }
  return { expected: true, opened: true, closed: Boolean(closed.matched), escapes, focus };
}

/**
 * What the GUI did instead of attaching, read when the attach receipt never came: every native
 * receipt line, the saved session as the GUI left it, the GUI's boot trace and one screenshot.
 * Read-only; a read that fails is recorded in place of its value.
 */
export async function attachDiagnostics(ctx, sessionFile) {
  const read = async (file) => { try { return await ctx.agent.call('readFile', { file }); } catch (error) { return `READ_FAILED: ${error?.message ?? error}`; } };
  const receipts = hjoin(path.posix.dirname(ctx.layout.barriers.replace(/\\/g, '/')), 'receipts');
  const out = {
    nativeEvents: await read(nativeEventsFile(ctx.layout)),
    nativeCurrent: await read(hjoin(receipts, 'native-current.json')),
    sessionState: await read(sessionFile),
    bootTrace: await read(hjoin(ctx.layout.runtime, 'boot-trace.log')),
    // A first v3 save over the seeded v2 file leaves session_state.json.pre-v3 next to it.
    sessionsDir: await ctx.agent.call('exec', { command: 'node', args: ['-e', 'try{console.log(JSON.stringify(require("fs").readdirSync(process.argv[1])))}catch(e){console.log(JSON.stringify(String(e)))}', ctx.layout.sessions] }).then((r) => r.stdout?.trim() ?? null, (error) => `LIST_FAILED: ${error?.message ?? error}`),
    screenshot: null,
  };
  if (ctx.mahord) {
    try { out.screenshot = (await ctx.mahord.screenshot())?.toString('base64') ?? null; } catch (error) { out.screenshot = `SCREENSHOT_FAILED: ${error?.message ?? error}`; }
  }
  return out;
}

/**
 * The remote line appender Q11 types into. Windows SSH sessions run %COMSPEC% (cmd.exe,
 * ferryx_scope/ssh/helper.rs default_platform_login_shell), which has no cat, and ConPTY turns LF
 * into Ctrl+Enter (run-r6c), so the POSIX line never ran and the marker was typed onto it
 * (run-q11 only-q11-q13-r4: a file named ssh-input.logUNCERTAIN...). On Windows a PowerShell
 * loop appends each line read from the console, ends with CR, and prints a ready sentinel built
 * from two halves so the echoed command line cannot match it. Elsewhere: cat, unchanged.
 */
export const APPENDER_READY = 'Q11APPENDREADY';
export const APPENDER_READY_MS = 30_000;
export function appenderCommand(platform, file) {
  if (platform !== 'windows') return `cat >> ${file}\n`;
  return `powershell -NoProfile -Command "Write-Output ('Q11APPEND' + 'READY'); while ($true) { $l = [Console]::ReadLine(); if ($null -eq $l) { break }; Add-Content -LiteralPath '${file}' -Value $l }"${enterKey(platform)}`;
}

/**
 * Q11 marker. Windows: lowercase only. MahoRD on maho-win drops Shift+letter (run-int2-all Q11:
 * the first key "U" never echoed in two runs), and a plain key press is delivered.
 */
export function q11Marker(platform) {
  const marker = nonce('UNCERTAIN').replace('_', '');
  return platform === 'windows' ? marker.toLowerCase() : marker;
}

/** The appended lines with CRLF (Add-Content on Windows) read as LF. */
export function appendedText(text) {
  return String(text ?? '').replace(/\r\n/g, '\n');
}

export async function Q11(ctx, result) {
  if (!requireOwner(ctx, result)) return;
  if (!ctx.hooks.writeAckHoldArm?.present) return block(result, 'HOOK_MISSING', ctx.hooks.writeAckHoldArm.owner);
  if (!ctx.mahord) return block(result, 'GUI_INPUT_UNAVAILABLE', 'Q11 types the marker through the GUI with MahoRD');
  ctx.sshd = ctx.sshd ?? await startSshd(ctx);
  if (ctx.sshd.unavailable) return block(result, 'SSHD_UNAVAILABLE', ctx.sshd.unavailable);
  const conn = ctx.state.conn;
  // An earlier scenario that failed before handing back a data connection leaves none.
  if (!conn) return block(result, 'NO_DATA_CONNECTION', 'no data connection from the previous scenario');
  const spawned = await conn.call(D.spawnSsh({ clientRequestId: `qa-q11-${nonce('R')}`, workspaceId: ctx.sshd.workspaceId, hostStorePath: ctx.sshd.hostStorePath }), 60_000);
  assertion(result, 'SSH session spawned against the private sshd', spawned.type === 'spawnOk', spawned);
  if (spawned.type !== 'spawnOk') return;
  await recordSshHelper(ctx);
  const sid = spawned.sessionId;
  const incarnation = spawned.session?.incarnation ?? null;
  const input = hjoin(ctx.sshd.repoRoot, 'ssh-input.log');
  await conn.call(D.attach(sid, 0));
  const appenderSince = await conn.outputLength(sid);
  await conn.call(D.write(sid, appenderCommand(ctx.platform, input)));
  if (ctx.platform === 'windows') {
    const ready = await conn.waitOutput(sid, APPENDER_READY, APPENDER_READY_MS, { since: appenderSince });
    if (!assertion(result, 'the remote line appender is running in the SSH session', !ready.timedOut, { timedOut: Boolean(ready.timedOut) })) return;
  }
  const before = await conn.describe();
  const sBefore = before.sessions.find((s) => s.backendId === sid);
  await evidence(ctx, result, 'before.json', before);
  assertion(result, 'SSH session live with a host-scoped remote pid', sBefore?.connected && sBefore?.remotePid != null && sBefore?.localChildPid == null, sBefore);
  // Plan line 225: the marker is typed through the GUI. The GUI reaches an existing SSH backend
  // only through its own restore path: a saved session whose SSH workspace holds a tab bound to
  // that backend id (App.tsx recoverProjectBootstrap; sessionPersistence.ts keeps an SSH
  // backendSessionId and re-admits it). Quit the running GUI first so its save cannot overwrite it.
  if (ctx.state.gui) { await ctx.gui.quit(ctx.state.gui); ctx.state.gui = null; }
  const sessionFile = hjoin(ctx.layout.sessions, 'session_state.json');
  const existingText = await ctx.agent.call('readFile', { file: sessionFile });
  let existing = null;
  try { existing = existingText ? JSON.parse(existingText) : null; } catch { existing = null; }
  const seeded = sshRestoreSession(existing, {
    workspaceId: ctx.sshd.workspaceId, hostId: ctx.sshd.hostId, repoRoot: ctx.sshd.repoRoot,
    backendSessionId: sid, daemonEpoch: String(spawned.epoch), incarnation,
    localSessionId: `q11-pane-${ctx.runId}`, tabId: `q11-tab-${ctx.runId}`, now: Date.now(),
  });
  await ctx.agent.call('mkdir', { dir: ctx.layout.sessions });
  await ctx.agent.call('writeFile', { file: sessionFile, content: JSON.stringify(seeded) });
  await evidence(ctx, result, 'restore-session.json', seeded);
  // The operation id lets the GUI write its native attach receipts (qa_barrier.rs validate_native_tuple).
  const op = `q11-${ctx.runId}`;
  const pane = `q11-pane-${ctx.runId}`;
  const guiEnv = { FERRYX_QA_OPERATION_ID: op };
  if (expectsSetupWizard(ctx.platform)) {
    // Rust std::env::temp_dir (GetTempPath2) reads TMP, then TEMP: the GUI's switch-debug log lands here.
    await ctx.agent.call('mkdir', { dir: guiTempDir(ctx.layout) });
    guiEnv.TMP = guiTempDir(ctx.layout);
    guiEnv.TEMP = guiTempDir(ctx.layout);
  }
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts.A, { env: guiEnv });
  const wizard = await dismissSetupWizard(ctx, ctx.state.gui, pane);
  if (wizard.expected && !assertion(result, 'the first-run setup wizard opened and was closed before any key was typed', wizard.opened && wizard.closed, wizard)) return;
  const attached = await waitRestoredAttach(ctx, sid, ctx.state.gui);
  let tuple = null;
  try { tuple = attached.matched ? JSON.parse(attached.matched).attachTuple : null; } catch { tuple = null; }
  if (!assertion(result, 'a GUI tab is attached to the SSH session (native attach receipt)', tuple?.backendSessionId === sid && tuple?.incarnation === incarnation, { tuple, incarnation, attached })) {
    await evidence(ctx, result, 'attach-diagnostics.json', await attachDiagnostics(ctx, sessionFile));
    return;
  }
  // Type the marker into that tab. The remote tty echoes it, which proves the keystrokes reached
  // this session; cat has not read it yet (canonical mode holds the line until Enter). MahoRD drops
  // and repeats keys on omarchy (lib/typeverified.mjs), so the echoed line is driven to the marker
  // key by key and a surplus key is erased; the tty's erase echo means the raw output need not hold
  // the marker contiguously, so the rendered input line is what must equal it.
  const marker = q11Marker(ctx.platform);
  // Windows: ConPTY repaints the whole screen per key (ESC[H), so the typed line is the row after
  // the appender's ready line in the last repaint (renderConptyInput), read from before that line.
  const conpty = ctx.platform === 'windows';
  const since = conpty ? appenderSince : await conn.outputLength(sid);
  const render = conpty ? (raw) => renderConptyInput(raw, APPENDER_READY) : renderEcho;
  // Keys go to the foreground window: make it the GUI that attached (Windows: gui.focus).
  const focused = await ctx.gui.focus(ctx.state.gui);
  await evidence(ctx, result, 'focus.json', { beforeEscape: attached.focus ?? null, beforeTyping: focused });
  let typed;
  try {
    typed = await typeVerified({
      mahord: ctx.mahord,
      text: marker,
      render,
      echo: async () => String(await conn.outputOf(sid) ?? '').slice(since),
      waitMore: (length, ms) => conn.waitOutput(sid, '', ms, { since: since + length }),
    });
  } catch (error) {
    typed = { error: error.detail ?? String(error?.message ?? error) };
  }
  const echoed = render(String(await conn.outputOf(sid) ?? '').slice(since));
  if (!assertion(result, 'the marker typed through the GUI reached the SSH session', !typed.error && echoed === marker, { typed, echoed })) return;
  // Arm the hold only now: the GUI input queue is serialized, so a held keystroke would block the
  // rest of the marker. Enter is the uncertain write that commits the marker line to cat.
  const receipts = hjoin(ctx.layout.barriers, 'write-ack-hold.receipt.jsonl');
  await ctx.agent.call('writeFile', { file: hjoin(ctx.layout.barriers, 'write-ack-hold.request.json'), content: JSON.stringify({ runId: ctx.runId, operationId: op, sessionId: sid }) });
  const armed = await ctx.agent.call('waitLog', { file: receipts, pattern: `"armed"[^\\n]*"${sid}"`, deadlineMs: 10_000 });
  assertion(result, 'write-ack hold armed for the SSH session', Boolean(armed.matched), armed);
  // Enter can be dropped like any MahoRD key; one that reached the GUI shows as write-applied
  // within seconds, so a missing receipt means a lost key and Enter is resent (bounded). A second
  // Enter that did land would show as an extra line, which the exact remote-log check reports.
  let applied = { matched: null };
  let enterSends = 0;
  while (!applied.matched && enterSends < 3) {
    enterSends += 1;
    await ctx.mahord.key('Enter');
    applied = await ctx.agent.call('waitLog', { file: receipts, pattern: `"write-applied"[^\\n]*"${sid}"`, deadlineMs: 10_000 });
  }
  applied.enterSends = enterSends;
  assertion(result, 'write-applied observed for the GUI write while the ack is held', Boolean(applied.matched), applied);
  const atQuit = (await ctx.agent.call('readFile', { file: receipts })) ?? '';
  assertion(result, 'the ack was still held when the GUI quit', Boolean(applied.matched) && !new RegExp(`"released"[^\\n]*"${sid}"`).test(atQuit), atQuit);
  const quit = await ctx.gui.quit(ctx.state.gui);
  ctx.state.gui = null;
  await ctx.agent.call('writeFile', { file: hjoin(ctx.layout.barriers, 'write-ack-hold.release.json'), content: JSON.stringify({ runId: ctx.runId, operationId: op, sessionId: sid }) });
  const released = await ctx.agent.call('waitLog', { file: receipts, pattern: `"released"[^\\n]*"${sid}"`, deadlineMs: 10_000 });
  assertion(result, 'write-ack hold released after the GUI quit', Boolean(released.matched), released);
  ctx.state.gui = await ctx.gui.launch(ctx.artifacts.A);
  const conn2 = await dataConn(ctx);
  const after = await conn2.describe();
  const sAfter = after.sessions.find((s) => s.backendId === sid);
  await evidence(ctx, result, 'after.json', { after, quit });
  assertion(result, 'same backend id and remote_pid after GUI quit/relaunch', sAfter?.remotePid === sBefore?.remotePid && isLive(sAfter), sAfter);
  await interrupt(conn2, sid);
  // The private sshd listens on the agent's own host (127.0.0.1), so the file the SSH session
  // appended to is read where it lies; no shell-specific read command (cmd.exe has no cat).
  const raw = await ctx.agent.call('readFile', { file: input });
  const log = { stdout: appendedText(raw) };
  await evidence(ctx, result, 'ssh-input.log', raw ?? '');
  const count = log.stdout.split(marker).length - 1;
  assertion(result, 'remote log contains the marker exactly once (no replay of the uncertain write)', count === 1, count);
  assertion(result, 'remote log holds only the committed marker line (the held Enter was not replayed)', log.stdout === `${marker}\n`, JSON.stringify(log.stdout));
  await conn2.call(D.close(sid));
  await ctx.state.conn.close().catch(() => {});
  ctx.state.conn = conn2;
}

export async function Q12(ctx, result) {
  if (!requireOwner(ctx, result)) return;
  const conn = ctx.state.conn;
  if (!conn) return block(result, 'NO_DATA_CONNECTION', 'no data connection from the previous scenario');
  // exited local entry
  const spawned = await conn.call(D.spawnLocal({ clientRequestId: `qa-q12-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir }));
  const exited = spawned.sessionId;
  await conn.call(D.attach(exited, 0));
  // Prompt first: exit typed while the shell is still starting can be lost (common.mjs seedLocalSession).
  const prompt = await conn.waitOutput(exited, null, SHELL_READY_TIMEOUT_MS, { visible: true });
  assertion(result, 'exited-entry shell printed its prompt before exit was typed', !prompt.timedOut, { timedOut: Boolean(prompt.timedOut), bytes: String(prompt.text ?? '').length });
  const child = (await conn.describe()).sessions.find((s) => s.backendId === exited)?.localChildPid;
  // Subscribe to the shell's exit before asking it to exit, then read the inventory once.
  const gone = child ? ctx.agent.call('waitExit', { pid: child, deadlineMs: 15_000 }) : Promise.resolve({ timedOut: true });
  // The daemon removes the entry only after its lifecycle poll and reader shutdown (pty.rs:19-20,
  // finalize_natural_exit); the stream's Exit message (server.rs:7666) is sent once the session's
  // output channel closes, so the inventory is read after that event, not after the child pid.
  const ended = conn.waitSessionExit(exited, SESSION_EXIT_TIMEOUT_MS);
  await conn.call(D.write(exited, `exit${enterKey(ctx.platform)}`));
  const childExit = await gone;
  assertion(result, 'exited session child process is gone', !childExit.timedOut, { child, childExit });
  const streamExit = await ended;
  assertion(result, 'attached stream delivered the session Exit message', !streamExit.timedOut, streamExit);
  const desc = await conn.describe();
  const entry = desc.sessions.find((s) => s.backendId === exited);
  assertion(result, 'exited local PTY reports readerOpen=false (or is gone)', !entry || entry.readerOpen === false, entry);
  assertion(result, 'exited entry is not counted live', !liveSessions(desc).some((s) => s.backendId === exited), null);
  // Disconnected SSH entry. Stopping the recorded sshd listener does not necessarily drop an
  // established connection, so the assertion is the predicate itself on whatever state the daemon
  // reports: an entry is live exactly when connected is true, and its remote pid is never local.
  if (ctx.sshd && !ctx.sshd.unavailable) {
    const ssh = await conn.call(D.spawnSsh({ clientRequestId: `qa-q12s-${nonce('R')}`, workspaceId: ctx.sshd.workspaceId, hostStorePath: ctx.sshd.hostStorePath }), 60_000);
    if (ssh.type === 'spawnOk') {
      await ctx.agent.call('kill', { pid: ctx.sshd.pid, signal: 'SIGTERM' });
      const rows = (await conn.describe()).sessions;
      const s = rows.find((x) => x.backendId === ssh.sessionId);
      await evidence(ctx, result, 'disconnected.json', s ?? null);
      assertion(result, 'SSH entry is live exactly when connected', !s || isLive(s) === (s.connected === true), s);
      assertion(result, 'a disconnected SSH entry is excluded from Inventory', !s || s.connected || !liveSessions(await conn.describe()).some((x) => x.backendId === s.backendId), s);
      assertion(result, 'remote_pid never placed in local_child_pid', !s || s.localChildPid == null, s);
      ctx.sshd = null;
    }
  }
  await evidence(ctx, result, 'describe.json', await conn.describe());
  const ctl = await controlConn(ctx);
  const inv = await ctl.inventory();
  const live = liveSessions(await conn.describe());
  assertion(result, 'control Inventory live_local equals the live local entries', inv.live_local === live.filter((s) => s.kind === 'local').length, { inv, live: live.length });
  await ctl.close();
}

/**
 * Q13 readiness. A daemon.sock path proves nothing: the retired owner's socket file can outlive it
 * (r3c: ECONNREFUSED right after the path check). The baseline prints FERRYX_DAEMON_READY only
 * after its listener is bound (cli.rs, ready_rx), so wait for that line in the job's stdout log
 * (fs.watch event, bounded), then require one accepted protocol-5 handshake from a new pid.
 */
export async function awaitLegacyReady(ctx, { retiredPid = null } = {}) {
  const logs = legacyLogFiles(ctx.layout);
  const announced = await ctx.agent.call('waitLog', { file: logs.stdout, pattern: '^FERRYX_DAEMON_READY$', flags: 'm', deadlineMs: DAEMON_READY_TIMEOUT_MS });
  const stderr = async () => (await ctx.agent.call('readFile', { file: logs.stderr })) ?? null;
  if (!announced.matched) return { conn: null, reason: 'NO_READY_LINE', logs, stderr: await stderr() };
  let conn;
  try {
    conn = await dataConn(ctx, ctx.layout.runtime, { version: 5 });
  } catch (error) {
    return { conn: null, reason: 'HANDSHAKE_FAILED', error: String(error?.message ?? error), logs, stderr: await stderr() };
  }
  const hs = conn.handshake;
  if (hs?.type !== 'handshakeOk' || hs.version !== 5 || !hs.pid || hs.pid === retiredPid) {
    await conn.close().catch(() => {});
    return { conn: null, reason: 'HANDSHAKE_NOT_LEGACY_V5', handshake: hs, logs };
  }
  return { conn, logs };
}

/** Bound of Q12's wait for the stream Exit message: lifecycle poll 250 ms + 2 x READER_SHUTDOWN_TIMEOUT 2 s (pty.rs:19-20), with margin. */
export const SESSION_EXIT_TIMEOUT_MS = 10_000;

const sameExe = (a, b) => String(a ?? '').toLowerCase() === String(b ?? '').toLowerCase();

/** Process-table rows of the legacy daemon: exactly the baseline executable, run with --daemon. */
export function legacyDaemonRows(rows, executable) {
  return rows.filter((r) => sameExe(r.executable, executable) && /--daemon(\s|$|")/.test(r.commandLine ?? '') && !/--daemon-supervisor/.test(r.commandLine ?? ''));
}

/**
 * Records the legacy job's daemon as soon as the OS job starts it: the one baseline --daemon row
 * that was not running before the install, recorded through the agent's identity check. The OS job
 * starts asynchronously, so the table is re-read every intervalMs up to deadlineMs.
 */
export async function recordLegacyJobProcess(ctx, executable, prior, { deadlineMs = 15_000, intervalMs = 500 } = {}) {
  const until = Date.now() + deadlineMs;
  for (;;) {
    const fresh = legacyDaemonRows(await ctx.agent.call('table'), executable).filter((r) => !prior.has(r.pid));
    if (fresh.length === 1) {
      try {
        return { recorded: await ctx.agent.call('record', { pid: fresh[0].pid, role: 'daemon-legacy', executable }) };
      } catch (error) {
        return { recorded: null, pid: fresh[0].pid, error: String(error?.message ?? error) };
      }
    }
    if (fresh.length > 1) return { recorded: null, ambiguous: fresh.map((r) => r.pid) };
    if (Date.now() >= until) return { recorded: null, reason: `no new ${executable} --daemon within ${deadlineMs}ms` };
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
}

/**
 * Closes ownId and every other session the daemon lists, then lists again. foreign: the sessions
 * this scenario did not spawn, with each close response.
 */
export async function closeAllSessions(conn, ownId) {
  const closed = [{ id: ownId, resp: await conn.call(D.close(ownId)) }];
  const before = await conn.call(D.listSessions());
  const foreign = [];
  for (const id of before.sessions ?? []) {
    if (id === ownId) continue;
    foreign.push({ id, resp: await conn.call(D.close(id)) });
  }
  return { closed, foreign, before, after: foreign.length ? await conn.call(D.listSessions()) : before };
}

export async function Q13(ctx, result) {
  if (!ctx.artifacts.baseline) return block(result, 'ARTIFACT_MISSING', 'pre-plan baseline daemon binary not built');
  const retired = await retireOwner(ctx);
  assertion(result, 'QA generation owner retired', retired.retired, retired);
  // The pre-plan baseline only reads a version-1 catalog; the HEAD owners wrote version 2.
  await evidence(ctx, result, 'catalog-set-aside.json', await setAsideHeadCatalog(ctx, 'Q13'));
  const legacyEnv = { FERRYX_DATA_DIR: ctx.layout.data, FERRYX_RUNTIME_DIR: ctx.layout.runtime, FERRYX_SESSION_DIR: ctx.layout.sessions };
  const legacyExecutable = ctx.artifacts.baseline.daemonExecutable;
  const priorLegacy = new Set(legacyDaemonRows(await ctx.agent.call('table'), legacyExecutable).map((r) => r.pid));
  const installed = await ctx.jobs.installLegacy({ executable: legacyExecutable, env: legacyEnv });
  await evidence(ctx, result, 'legacy-install.json', installed);
  if (!assertion(result, 'legacy QA job installed and started', installed?.status === 0, installed)) return;
  // Recorded at job start, before readiness: a daemon that never gets ready is still this run's to
  // end (run-q00fix-all left pid 3856 behind because it was recorded only after the handshake).
  const started = await recordLegacyJobProcess(ctx, legacyExecutable, priorLegacy);
  await evidence(ctx, result, 'legacy-recorded.json', started);
  const ready = await awaitLegacyReady(ctx, { retiredPid: retired.pid });
  if (!ready.conn) {
    await evidence(ctx, result, 'legacy-not-ready.json', { ...ready, job: await ctx.jobs.inspect(null).catch((e) => String(e)) });
    assertion(result, 'legacy daemon accepted a protocol-5 handshake', false, ready);
    return;
  }
  const conn = ready.conn;
  assertion(result, 'legacy daemon accepted a protocol-5 handshake', true, conn.handshake);
  const legacyPid = conn.handshake.pid;
  const legacyExe = (await ctx.agent.call('probe', { pid: legacyPid }))?.executable;
  await ctx.agent.call('record', { pid: legacyPid, role: 'daemon-legacy', executable: legacyExe });
  assertion(result, 'legacy daemon has no control endpoint', (await controlConn(ctx)) === null, null);
  const reg = await conn.call(D.registerWorkspace(ctx.workspaceId, ctx.repoDir));
  if (!assertion(result, 'legacy daemon registered the QA workspace', reg?.type === 'registerWorkspaceOk', reg)) return;
  const sp = await conn.call(D.spawnLocal({ clientRequestId: `qa-q13-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir }));
  if (!assertion(result, 'legacy daemon spawned a live session', sp.type === 'spawnOk', sp)) return;
  const legacyBefore = await ctx.agent.call('probe', { pid: legacyPid });
  // New GUI meets the legacy owner.
  const gui = await ctx.gui.launch(ctx.artifacts.A);
  ctx.state.gui = gui;
  const guiLog = hjoin(ctx.layout.logs, `gui-${gui.tag}.log`);
  await ctx.agent.call('waitLog', { file: guiLog, pattern: 'legacy|attached|handshake', flags: 'i', deadlineMs: DAEMON_READY_TIMEOUT_MS });
  await conn.call(D.attach(sp.sessionId, 0));
  // Spawned just above with nothing typed into it: the only session that can be before its first prompt.
  await nonceRoundTrip(ctx, conn, sp.sessionId, result, 'legacy session usable with the new GUI attached', { freshShell: true });
  const legacyAfter = await ctx.agent.call('probe', { pid: legacyPid });
  assertion(result, 'legacy pid and start time unchanged', legacyBefore && legacyAfter && String(legacyBefore.startTime) === String(legacyAfter.startTime), { legacyBefore, legacyAfter });
  const legacyJob = await ctx.jobs.inspect(null);
  const text = (await ctx.agent.call('readFile', { file: guiLog })) ?? '';
  await evidence(ctx, result, 'gui.log', text);
  await evidence(ctx, result, 'legacy-job.json', legacyJob);
  assertion(result, 'no bootout/stop/End sent to the legacy job', !new RegExp(`(bootout|stop|/End|/Delete)[^\\n]*${ctx.jobs.base}(?![.-])`).test(text), null);
  assertion(result, 'no UpgradeBinary sent to the legacy owner', !/upgradeBinary|UpgradeBinary/.test(text), null);
  if (ctx.platform === 'windows') {
    // Carry-over todo 14: the real Get-ScheduledTask State read on maho-win.
    assertion(result, 'legacy task State read for real (Running)', legacyJob.state === 'Running', legacyJob);
    ctx.carry.push({ item: 'todo 14: Get-ScheduledTask State read of the legacy task on maho-win', evidence: path.posix.join(ctx.evidenceDir, 'Q13-legacy-job.json') });
  } else {
    const reg = ctx.platform === 'mac'
      ? /<key>RunAtLoad<\/key>\s*<false\/>/.test((await ctx.agent.call('readFile', { file: hjoin(ctx.layout.launchAgents, `${ctx.jobs.base}.plist`) })) ?? '')
      : legacyJob.unitFileState !== 'enabled';
    assertion(result, 'legacy job lost login autostart without being stopped', reg, legacyJob.unitFileState ?? null);
  }
  // Zero-session explicit end of the legacy daemon (stand-in for logout/reboot).
  // Earlier scenarios' sessions live in the run's session store and follow it into the legacy owner
  // (run-all-fix all2: Q08's SSH pair fd805ced/717906d9, listed again here once Q08 completed instead
  // of throwing; its exception recovery used to close them). Every listed session is this run's.
  const drained = await closeAllSessions(conn, sp.sessionId);
  await evidence(ctx, result, 'legacy-drain.json', drained);
  const listed = drained.after;
  const desc = await conn.call(D.describeLiveSessions()).catch(() => null);
  const zero = (desc?.type === 'describeLiveSessionsOk' ? liveSessions(desc).length : listed.sessions?.length ?? -1) === 0;
  assertion(result, 'legacy live count is zero before the explicit shutdown', zero, { listed, desc, foreign: drained.foreign });
  if (!zero) return;
  await ctx.gui.quit(ctx.state.gui);
  const exit = ctx.agent.call('waitExit', { pid: legacyPid, deadlineMs: 30_000 });
  await conn.call(D.shutdown()).catch(() => null);
  const exited = await exit;
  assertion(result, 'legacy daemon exited after the zero-session Shutdown', !exited.timedOut, exited);
  const fresh = await freshOwner(ctx, ctx.artifacts.A, { expectKey: ctx.keys.A });
  const jobs = await ctx.jobs.listQaJobs();
  await evidence(ctx, result, 'after-migration.json', { fresh, jobs });
  assertion(result, 'newest generation started on the next admission', fresh.hello?.generation_key === ctx.keys.A, fresh.hello);
  assertion(result, 'legacy plist/unit/task removed', !jobs.some((j) => j === ctx.jobs.jobName(null) || j === `${ctx.jobs.base}.service`), jobs);
  void echoCommand;
}
