// Shared scenario context helpers for the daemon-lifetime QA harness.

import path from 'node:path';
import { hjoin } from '../lib/hostpath.mjs';
import { randomBytes } from 'node:crypto';
import { data as D, control as C, liveSessions, compareInventories, identityOf, isLive, visibleText } from '../lib/wire.mjs';
import { assertion, block } from '../lib/summary.mjs';

export const SUPERVISED_START_WAIT_MS = 5_000; // client.rs SUPERVISED_START_WAIT
export const DAEMON_READY_TIMEOUT_MS = 30_000; // client.rs DAEMON_READY_TIMEOUT
export const RESPAWN_BACKOFF_MS = 1_000; // supervisor_windows.rs RESPAWN_BACKOFF
/** owner_admission.rs starting_window_ms(): a younger starting.json is a pending start (row 5b). */
export const STARTING_WINDOW_MS = DAEMON_READY_TIMEOUT_MS * 2;

/** A QA owner the harness cannot adopt; code is OWNER_GONE, OWNER_UNCONFIRMED or OWNER_STATE_UNRECOVERABLE. */
export class OwnerStateError extends Error {
  constructor(code, detail) {
    super(`${code}: ${JSON.stringify(detail)}`);
    this.name = 'OwnerStateError';
    this.code = code;
    this.detail = detail;
  }
}

export function nonce(prefix = 'QA') {
  return `${prefix}_${randomBytes(6).toString('hex').toUpperCase()}`;
}

/**
 * Line terminator for input typed into a session's shell. Windows: Enter is CR alone. ConPTY turns
 * each input byte into a key event (InputStateMachineEngine::_DoControlCharacter), so a trailing
 * LF arrives as a second key, Ctrl+Enter, which PSReadLine 2.0 binds to InsertLineAbove: it stays
 * queued while a command runs and, once Ctrl+C returns to a prompt, opens an empty continuation
 * line (`>>`). The next typed command is then appended under it and its CR only adds another line,
 * so it never runs. Measured on maho-win: w5-qa/windows/run-r6c/q01-probe/result.json
 * (CR+LF: `>>`, nonce not echoed; CR: no `>>`, nonce echoed once).
 */
export function enterKey(platform) {
  return platform === 'windows' ? '\r' : '\n';
}

/** Shell line that prints a counter every 100ms, per platform. */
export function counterCommand(platform, tag) {
  // Windows sessions are spawned with seedShell('windows') = powershell (see below), so this
  // PowerShell syntax always meets the shell it was written for.
  if (platform === 'windows') return `for($i=0;;$i++){"${tag}_TICK_$i";Start-Sleep -Milliseconds 100}${enterKey(platform)}`;
  return `i=0; while :; do i=$((i+1)); echo ${tag}_TICK_$i; sleep 0.1; done${enterKey(platform)}`;
}

/**
 * Shell named in the harness's local Spawn. With shell null the daemon picks pwsh.exe when it is on
 * PATH, else powershell.exe (src-tauri/src/terminal/shell.rs:315-319); cmd.exe only on an explicit
 * "cmd" preference (shell.rs:302-305). The request's shell field (daemon/protocol.rs:535) goes
 * straight to resolve_startup_command (daemon/session_service.rs:2442-2443), so naming powershell
 * pins powershell.exe (shell.rs:298-301) and the Windows commands above never depend on PATH.
 */
export function seedShell(platform) {
  return platform === 'windows' ? 'powershell' : null;
}

/** Wait for a fresh shell's first output (prompt), and for the counter's second tick. */
export const SHELL_READY_TIMEOUT_MS = 30_000;
export const COUNTER_TIMEOUT_MS = 15_000;
/** Received-text tail kept in a seed failure. */
export const SEED_TEXT_TAIL = 4_000;

export class SeedError extends Error {
  constructor(message, detail) {
    super(`${message}: ${JSON.stringify(detail)}`);
    this.code = 'SEED_FAILED';
    this.detail = detail;
  }
}

export function echoCommand(platform, marker) {
  const half = Math.floor(marker.length / 2);
  // Split so the echoed input line never contains the marker itself; only the output does.
  if (platform === 'windows') return `Write-Output ('${marker.slice(0, half)}' + '${marker.slice(half)}')${enterKey(platform)}`;
  return `printf '%s%s\\n' '${marker.slice(0, half)}' '${marker.slice(half)}'${enterKey(platform)}`;
}

/** Writes raw evidence for a scenario and links it into the result. */
export async function evidence(ctx, result, name, content) {
  const file = path.posix.join(ctx.evidenceDir, `${result.id}-${name}`);
  const text = typeof content === 'string' ? content : JSON.stringify(content, null, 2);
  await ctx.writeEvidence(file, text);
  result.evidence.push(file);
  return file;
}

/** One data connection kept for the scenario; closed by the caller. */
export async function dataConn(ctx, runtimeDir = ctx.layout.runtime, opts = {}) {
  const opened = await ctx.agent.call('dataOpen', { runtimeDir, ...opts });
  const call = (frame, deadlineMs) => ctx.agent.call('call', { handle: opened.handle, frame, deadlineMs });
  return {
    handle: opened.handle,
    handshake: opened.handshake,
    call,
    describe: async () => (await call(D.describeLiveSessions())),
    waitOutput: (sessionId, marker, deadlineMs, { visible = false, promptAfter = null, since = 0 } = {}) => ctx.agent.call('waitOutput', { handle: opened.handle, sessionId, marker, visible, promptAfter, since, deadlineMs }),
    outputOf: (sessionId) => ctx.agent.call('outputOf', { handle: opened.handle, sessionId }),
    outputLength: async (sessionId) => (await ctx.agent.call('outputOf', { handle: opened.handle, sessionId })).length,
    waitSessionExit: (sessionId, deadlineMs) => ctx.agent.call('waitSessionExit', { handle: opened.handle, sessionId, deadlineMs }),
    close: () => ctx.agent.call('close', { handle: opened.handle }),
  };
}

export async function controlConn(ctx, runtimeDir = ctx.layout.runtime) {
  const opened = await ctx.agent.call('controlOpen', { runtimeDir });
  if (opened.handle === null) return null;
  const call = (frame, deadlineMs) => ctx.agent.call('call', { handle: opened.handle, frame, deadlineMs });
  return {
    handle: opened.handle,
    hello: () => call(C.hello()),
    inventory: () => call(C.inventory()),
    requestUpgrade: (exe, key) => call(C.requestUpgrade(exe, key), 120_000),
    retireWhenIdle: () => call(C.retireWhenIdle()),
    shutdown: (n) => call(C.shutdown(n)),
    waitClosed: (deadlineMs) => ctx.agent.call('waitClosed', { handle: opened.handle, deadlineMs }),
    close: () => ctx.agent.call('close', { handle: opened.handle }),
  };
}

/** Owner identity right now: control Hello pid + OS start time, cross-checked with owner.json. */
export async function ownerIdentity(ctx, runtimeDir = ctx.layout.runtime) {
  const ctl = await controlConn(ctx, runtimeDir);
  const hello = ctl ? await ctl.hello() : null;
  if (ctl) await ctl.close();
  const ownerJson = await ctx.agent.call('readFile', { file: hjoin(runtimeDir, 'owner.json') });
  const owner = ownerJson ? JSON.parse(ownerJson) : null;
  // The raw serialized field: a Windows FILETIME exceeds 2^53, so JSON.parse would round it.
  const ownerStartTime = ownerJson ? (/"startTime"\s*:\s*(\d+)/.exec(ownerJson)?.[1] ?? null) : null;
  const pid = hello?.pid ?? owner?.pid ?? null;
  const os = pid ? await ctx.agent.call('probe', { pid }) : null;
  return { pid, hello, owner, ownerStartTime, os, platform: ctx.platform };
}

const LSTART_MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/**
 * Whether owner.json's startTime (owner_admission.rs process_start_time, decimal string) and the
 * agent's probe (proc.mjs probeIdentity) name the same process start. Units per platform:
 *   linux   - both /proc/<pid>/stat starttime clock ticks: exact.
 *   windows - both CreationTime FILETIME (100ns since 1601; Get-Process StartTime.ToFileTimeUtc): exact.
 *   mac     - record is microseconds since the Unix epoch (pbi_start_tvsec*1e6+usec); the probe is
 *             `ps -o lstart` in host-local time at 1s resolution (the mac agent runs on this host),
 *             so the record's whole seconds must equal the parsed lstart.
 * Returns false for a missing or unparseable value: an unverifiable start time is no proof.
 */
export function ownerStartMatches(platform, recorded, probed) {
  if (recorded === null || recorded === undefined || !/^\d+$/.test(String(recorded))) return false;
  if (probed === null || probed === undefined) return false;
  if (platform === 'linux' || platform === 'windows') return /^\d+$/.test(String(probed)) && BigInt(recorded) === BigInt(probed);
  if (platform === 'mac') {
    const m = /^\w{3} (\w{3})\s+(\d+) (\d+):(\d+):(\d+) (\d{4})$/.exec(String(probed).trim());
    const month = m ? LSTART_MONTHS.indexOf(m[1]) : -1;
    if (month < 0) return false;
    const seconds = new Date(Number(m[6]), month, Number(m[2]), Number(m[3]), Number(m[4]), Number(m[5])).getTime() / 1000;
    return BigInt(recorded) / 1_000_000n === BigInt(seconds);
  }
  return false;
}

/**
 * An owner is adoptable only when its recorded pid runs (OS probe). owner.json outlives its
 * daemon, so the record alone never proves liveness. A control Hello is not required: the daemon
 * spawns owner.json publication (server.rs spawn_owner_publication) before it binds control
 * (server.rs bind_control_endpoint), so a fresh owner may not answer Hello yet. A Hello that does
 * answer must name the same pid. When owner.json names that pid, its recorded startTime must match
 * the probed start time (ownerStartMatches): a reused pid running another process is not the owner.
 */
export function confirmOwner(identity) {
  if (!identity?.pid) return { ok: false, code: 'OWNER_GONE', detail: { pid: null, reason: 'no owner record and no control Hello' } };
  if (!identity.os) return { ok: false, code: 'OWNER_GONE', detail: { pid: identity.pid, hello: Boolean(identity.hello), reason: 'owner pid has no live process' } };
  if (identity.hello && identity.hello.pid !== identity.pid) return { ok: false, code: 'OWNER_UNCONFIRMED', detail: { pid: identity.pid, helloPid: identity.hello.pid ?? null, reason: 'control Hello answered for another pid' } };
  if (identity.owner?.pid === identity.pid && !ownerStartMatches(identity.platform, identity.ownerStartTime, identity.os.startTime)) {
    return { ok: false, code: 'OWNER_UNCONFIRMED', detail: { pid: identity.pid, platform: identity.platform ?? null, recordedStartTime: identity.ownerStartTime ?? null, probedStartTime: identity.os.startTime ?? null, reason: 'owner.json startTime does not match the live process (pid reuse)' } };
  }
  return { ok: true, code: null, detail: { pid: identity.pid } };
}

/** owner_admission.rs StartingRecord (camelCase); unreadable is its own state, as in row 1. */
export function parseStarting(text) {
  if (text === null || text === undefined) return { state: 'none' };
  try {
    const record = JSON.parse(text);
    if (!Number.isInteger(record?.launcherPid) || !Number.isInteger(record?.startedAtUnixMs)) throw new Error('launcherPid/startedAtUnixMs missing');
    return { state: 'present', record, key: `${record.launcherPid}:${record.startedAtUnixMs}` };
  } catch (error) {
    return { state: 'unreadable', error: String(error?.message ?? error) };
  }
}

async function readStarting(ctx, runtimeDir) {
  return parseStarting(await ctx.agent.call('readFile', { file: hjoin(runtimeDir, 'starting.json') }));
}

/**
 * owner.json names no live owner. Decides Absent vs a pending start the way admission does
 * (owner_admission.rs classify_owner rows 5b/6) without comparing host and controller clocks:
 * a present starting.json is pending until the owner record changes or STARTING_WINDOW_MS has
 * passed since this observation; the same record still present after that window is older than
 * the window, so admission ignores it too. Expired keys are kept in ctx.state.expiredStarts so a
 * later call does not wait on the same record again.
 *   absent  - no pending start (none, withdrawn or expired)
 *   owner   - the pending start published owner pid
 *   unknown - unreadable record or a different start replaced it; never treated as retired
 */
export async function awaitPendingStart(ctx, fromPid, runtimeDir = ctx.layout.runtime) {
  const first = await readStarting(ctx, runtimeDir);
  if (first.state === 'none') return { state: 'absent' };
  if (first.state === 'unreadable') return { state: 'unknown', reason: `starting record unreadable: ${first.error}` };
  const expired = (ctx.state.expiredStarts ??= new Set());
  if (expired.has(first.key)) return { state: 'absent', expired: first.key };
  const changed = await ctx.agent.call('waitOwnerChange', { runtimeDir, fromPid, deadlineMs: STARTING_WINDOW_MS });
  if (changed.owner) return { state: 'owner', pid: changed.owner.pid, starting: first.record, seen: changed.seen };
  const again = await readStarting(ctx, runtimeDir);
  if (again.state === 'none') return { state: 'absent', withdrawn: first.key };
  if (again.state === 'present' && again.key === first.key) {
    expired.add(first.key);
    return { state: 'absent', expired: first.key };
  }
  if (again.state === 'unreadable') return { state: 'unknown', reason: `starting record unreadable: ${again.error}` };
  return { state: 'unknown', reason: `a start by launcher pid ${again.record.launcherPid} replaced the pending one`, starting: again.record };
}

/**
 * Drops the scenario's owner state after its owner retired: that connection is dead, and the
 * next owner has not seen this run's registerWorkspace (seedLocalSession registers again).
 */
export async function invalidateOwnerState(ctx) {
  const conn = ctx.state.conn;
  ctx.state.conn = null;
  ctx.state.daemon = null;
  ctx.state.localA = null;
  ctx.workspaceRegistered = false;
  // The daemon behind this handle exited; closing only releases the agent-side handle.
  if (conn) await conn.close().catch(() => {});
}

/**
 * A scenario that needs the run's owner connection blocks (OWNER_UNAVAILABLE) instead of throwing
 * on null when an earlier scenario retired that owner and could not restore one (Q07/Q10 in
 * run-q00fix-all dereferenced null). Returns true when the owner state is usable.
 */
export function requireOwner(ctx, result) {
  // conn and daemon are set together (freshOwner) and dropped together (invalidateOwnerState).
  if (ctx.state.conn) return true;
  block(result, 'OWNER_UNAVAILABLE', { conn: Boolean(ctx.state.conn), daemon: ctx.state.daemon?.pid ?? null, reason: ctx.state.ownerLost ?? 'no confirmed QA owner connection' });
  return false;
}

/** <FERRYX_DATA_DIR>/remote/machine-workspaces.v1.json (server.rs catalog_path, auth.rs canonical_remote_dir). */
export function workspaceCatalogFile(ctx) {
  return hjoin(ctx.layout.data, 'remote', 'machine-workspaces.v1.json');
}

/**
 * Before an older-revision daemon (v1bump, the pre-plan baseline) owns this run's data dir: the
 * HEAD owners of the earlier scenarios wrote the workspace catalog as version 2, which an older
 * daemon refuses (its workspace_catalog::load accepts version 1 only, deny_unknown_fields) and then
 * fails every registerWorkspace. A real older owner predates that write, so the catalog is set
 * aside into the run dir (kept as evidence) and the workspace is registered afresh.
 */
export async function setAsideHeadCatalog(ctx, label) {
  const file = workspaceCatalogFile(ctx);
  const text = await ctx.agent.call('readFile', { file });
  ctx.workspaceRegistered = false;
  if (text === null) return { file, setAside: null };
  const kept = hjoin(ctx.layout.root, 'set-aside', `${label}-machine-workspaces.v1.json`);
  await ctx.agent.call('writeFile', { file: kept, content: text });
  await ctx.agent.call('remove', { target: file });
  return { file, setAside: kept, version: (() => { try { return JSON.parse(text).version ?? null; } catch { return null; } })() };
}

/**
 * On a seed timeout: the bounded tail of what the session sent and the daemon's live sessions,
 * written as evidence and carried in the thrown error.
 */
async function seedFailure(ctx, conn, { tag, stage, sessionId, marker, waited, deadlineMs }) {
  const text = String(waited?.text ?? '');
  let describe;
  try { describe = await conn.describe(); } catch (error) { describe = { error: String(error?.message ?? error) }; }
  const detail = {
    tag, stage, sessionId, marker, deadlineMs, shell: seedShell(ctx.platform),
    dsrReplies: waited?.dsrReplies ?? null, replyErrors: waited?.replyErrors ?? [],
    receivedBytes: text.length, receivedTail: text.slice(-SEED_TEXT_TAIL), describe,
  };
  const file = path.posix.join(ctx.evidenceDir, `seed-${tag}-${stage}.json`);
  await ctx.writeEvidence(file, JSON.stringify(detail, null, 2));
  detail.evidence = file;
  return new SeedError(`counter ${tag}: ${stage} timed out`, detail);
}

/**
 * Registers the QA workspace once and spawns a local session running a counter. The counter is
 * written only after the shell's first visible output (its prompt), so input never races shell
 * startup. ConPTY's cursor query (ESC[6n) is answered by the agent's output route
 * (lib/agent.mjs createOutputRoute) and is not visible text, so it never counts as the prompt.
 */
export async function seedLocalSession(ctx, conn, tag) {
  if (!ctx.workspaceRegistered) {
    const reg = await conn.call(D.registerWorkspace(ctx.workspaceId, ctx.repoDir));
    if (reg.type !== 'registerWorkspaceOk') throw new Error(`registerWorkspace: ${JSON.stringify(reg)}`);
    ctx.workspaceRegistered = true;
  }
  const spawned = await conn.call(D.spawnLocal({ clientRequestId: `qa-${tag}-${nonce('R')}`, workspaceId: ctx.workspaceId, cwd: ctx.repoDir, shell: seedShell(ctx.platform) }), 60_000);
  if (spawned.type !== 'spawnOk') throw new Error(`spawn: ${JSON.stringify(spawned)}`);
  const attached = await conn.call(D.attach(spawned.sessionId, 0));
  if (attached.type !== 'attachOk') throw new Error(`attach: ${JSON.stringify(attached)}`);
  const ready = await conn.waitOutput(spawned.sessionId, '', SHELL_READY_TIMEOUT_MS, { visible: true });
  if (ready.timedOut) throw await seedFailure(ctx, conn, { tag, stage: 'shell-ready', sessionId: spawned.sessionId, marker: '', waited: ready, deadlineMs: SHELL_READY_TIMEOUT_MS });
  const write = await conn.call(D.write(spawned.sessionId, counterCommand(ctx.platform, tag)));
  if (write.type !== 'writeOk') throw new Error(`write: ${JSON.stringify(write)}`);
  const first = await conn.waitOutput(spawned.sessionId, `${tag}_TICK_2`, COUNTER_TIMEOUT_MS);
  if (first.timedOut) throw await seedFailure(ctx, conn, { tag, stage: 'counter', sessionId: spawned.sessionId, marker: `${tag}_TICK_2`, waited: first, deadlineMs: COUNTER_TIMEOUT_MS });
  return spawned.sessionId;
}

/** Interrupts the counter (Ctrl-C) so the session takes input again. */
export async function interrupt(conn, sessionId) {
  return conn.call({ type: 'write', sessionId, data: Buffer.from('\u0003').toString('base64') });
}

/** Bound on the shell redrawing its line editor after Ctrl-C. */
export const INTERRUPT_SETTLE_MS = 5_000;
/** zsh (and bash 5.1+) enable bracketed paste each time the line editor starts reading a line. */
export const LINE_EDITOR_READY = '\x1b[?2004h';

/**
 * Ctrl-C, then (Unix) the shell's line editor ready again before anything is typed. Input written
 * right after Ctrl-C reaches zsh while it handles the interrupt, which discards what is pending:
 * run-int2-all Q08 "reattached session" sent Ctrl-C at an idle prompt and the echo command at once,
 * the new prompt then received "rintf ..." without its "p" (zsh: command not found: rintf, nonce
 * count 0, Q08-nonce-NONCE_3564458615F7.json). The wait is for the line editor's own readiness
 * sequence after the Ctrl-C; a shell that never emits it costs the bound and proceeds as before.
 * Windows has its own fresh-prompt probe (nonceRoundTrip). Returns the wait, or null when none.
 */
export async function interruptSettled(ctx, conn, sessionId) {
  const since = ctx.platform === 'windows' ? 0 : String(await conn.outputOf(sessionId) ?? '').length;
  await interrupt(conn, sessionId);
  if (ctx.platform === 'windows') return null;
  return conn.waitOutput(sessionId, LINE_EDITOR_READY, INTERRUPT_SETTLE_MS, { since });
}

/** Wait for the shell's fresh prompt after Ctrl+C (Windows: PowerShell prompt; elsewhere none). */
export const PROMPT_AFTER_INTERRUPT_MS = 10_000;

/** Wait for a never-prompted fresh Windows shell's first prompt (pwsh cold start). */
export const FIRST_PROMPT_MS = 30_000;

/** Enter typed at the shell after Ctrl+C: its own fresh prompt is what the probe waits for. */
export const FRESH_PROMPT_PROBE = '\r';

/** True once the session's output has shown a PowerShell prompt line (`PS <path>>`) anywhere. */
export function hasShownPrompt(text) {
  return /^PS [^>\r\n]*>/m.test(visibleText(String(text ?? '')).replace(/\r/g, ''));
}

/**
 * Fresh nonce echoed exactly once in one session (input accepted, output delivered, no replay).
 * The daemon sends output only on a connection that attached the session (server.rs Attach ->
 * pump_session_stream); a data connection opened after a GUI quit/relaunch has no stream yet, so
 * attach first. The agent gives each attach its own stream connection (agent.mjs call()).
 *
 * freshShell: the caller spawned this session itself and typed nothing into it (Q13's legacy
 * spawn). Only such a shell can still be before its first prompt, where a Ctrl+C ends pwsh, so only
 * it waits for that prompt. Any other session already ran the harness's counter, so it prompted
 * long ago, but its replayed output need not show it: a reattach replays only the ring tail, all
 * counter ticks (run-q11 all Q01: tail QA_A_TICK_304..317, no PS line, the first-prompt wait
 * timed out). Those are interrupted, then probed: Enter typed after an offset, and the prompt it
 * prints after that offset is the shell reading a new line.
 */
export async function nonceRoundTrip(ctx, conn, sessionId, result, label, { freshShell = false } = {}) {
  const attached = await conn.call(D.attach(sessionId, 0));
  assertion(result, `${label}: attach accepted`, attached?.type === 'attachOk', attached?.type === 'attachOk' ? 'attachOk' : attached);
  let idleAtFirstPrompt = false;
  if (ctx.platform === 'windows' && freshShell && !hasShownPrompt(await conn.outputOf(sessionId))) {
    // A Ctrl+C that reaches pwsh before its first prompt ends the shell (run-r6d q13-probe
    // out-task-noprompt-30000: exit 270 ms after Ctrl+C, then SESSION_NOT_FOUND on the write),
    // which is Q13's "legacy session not found". Only a session that never prompted waits here.
    const first = await conn.waitOutput(sessionId, '', FIRST_PROMPT_MS, { promptAfter: 0 });
    assertion(result, `${label}: first prompt before Ctrl+C`, !first.timedOut, { timedOut: Boolean(first.timedOut), tail: visibleText(String(first.text ?? '')).slice(-200) });
    // Its first prompt means nothing runs: no counter to interrupt, and a Ctrl+C there echoes
    // nothing to wait on (run-r6d q13-only: empty tail, then the echo passed). Type directly.
    idleAtFirstPrompt = !first.timedOut;
  }
  if (!idleAtFirstPrompt) await interruptSettled(ctx, conn, sessionId);
  if (ctx.platform === 'windows' && !idleAtFirstPrompt) {
    // Type the echo only at a fresh prompt: the Ctrl+C has ended the counter and PowerShell is
    // reading a new line (run-r6b Q01/Q02 typed into the loop's tail and hit a `>>` line).
    const since = String(await conn.outputOf(sessionId) ?? '').length;
    await conn.call(D.write(sessionId, FRESH_PROMPT_PROBE));
    const prompt = await conn.waitOutput(sessionId, '', PROMPT_AFTER_INTERRUPT_MS, { promptAfter: since });
    assertion(result, `${label}: fresh prompt after Ctrl+C`, !prompt.timedOut, { timedOut: Boolean(prompt.timedOut), since, tail: visibleText(String(prompt.text ?? '').slice(since)).slice(-200) });
  }
  const marker = nonce('NONCE');
  const w = await conn.call(D.write(sessionId, echoCommand(ctx.platform, marker)));
  const out = await conn.waitOutput(sessionId, marker, 15_000);
  assertion(result, `${label}: write accepted`, w.type === 'writeOk', w);
  const passed = !out.timedOut && out.count === 1;
  const text = String(out.text ?? '');
  // On failure: did the tty echo the Ctrl-C and the typed command (input reached the terminal)?
  const failure = { count: out.count, timedOut: Boolean(out.timedOut), sawInterruptEcho: text.includes('^C'), sawCommandEcho: text.includes(marker.slice(0, Math.floor(marker.length / 2))), length: text.length, tail: text.slice(-600) };
  const ok = assertion(result, `${label}: nonce ${marker} echoed exactly once`, passed, passed ? { count: out.count, timedOut: false } : failure);
  if (!ok) {
    // Diagnosable failure (Q01 count 0 in run-q00fix-all): what the session sent, and the daemon's view.
    let describe;
    try { describe = await conn.describe(); } catch (error) { describe = { error: String(error?.message ?? error) }; }
    await evidence(ctx, result, `nonce-${marker}.json`, { label, sessionId, marker, dsrReplies: out.dsrReplies ?? null, receivedBytes: text.length, receivedTail: text.slice(-SEED_TEXT_TAIL), describe });
  }
  return marker;
}

/** Pre/post DescribeLiveSessions comparison with assertions. */
export function assertInventoryKept(result, before, after, label) {
  const problems = compareInventories(liveSessions(before), after.sessions);
  assertion(result, `${label}: every live session kept its identity`, problems.length === 0, problems);
  return problems.length === 0;
}

export function assertSameOwner(result, before, after, label) {
  const same = before.pid === after.pid && before.os && after.os && String(before.os.startTime) === String(after.os.startTime);
  assertion(result, `${label}: daemon pid and start time unchanged`, same, { before: { pid: before.pid, start: before.os?.startTime }, after: { pid: after.pid, start: after.os?.startTime } });
  return same;
}

export { identityOf, isLive, liveSessions };

/**
 * Ends the current owner the plan-sanctioned way for QA: quit the GUI, close every QA session,
 * then control RetireWhenIdle (zero live sessions) and await the owner's exit event.
 * A recorded owner without a live process is retired only when no start is pending
 * (awaitPendingStart); a pending start that publishes an owner is retired in turn, once.
 */
export async function retireOwner(ctx, { depth = 0 } = {}) {
  if (ctx.state.gui) { await ctx.gui.quit(ctx.state.gui).catch(() => {}); ctx.state.gui = null; }
  const owner = await ownerIdentity(ctx);
  // owner.json outlives its daemon: a recorded pid that no longer runs is retired only when no
  // start is pending, because a pending start publishes the next owner (row 5b).
  if (!owner.pid || !owner.os) {
    const pending = await awaitPendingStart(ctx, owner.pid);
    if (pending.state === 'absent') return { retired: true, pid: owner.pid, reason: owner.pid ? 'owner pid no longer runs' : 'no owner record', pending };
    if (pending.state === 'unknown') return { retired: false, pid: owner.pid, reason: pending.reason, pending };
    if (depth > 0) return { retired: false, pid: pending.pid, reason: 'another pending start published an owner while retiring', pending };
    return { ...(await retireOwner(ctx, { depth: depth + 1 })), pending };
  }
  // A fresh connection to the current owner: ctx.state.conn may still name a generation that has
  // since handed over (rerun2: Q08's after-record recovery made B the owner while the scenario
  // connection was C's), and closing through it leaves the owner's sessions open, so
  // RetireWhenIdle answers live:1 and never exits.
  if (ctx.state.conn) await ctx.state.conn.close().catch(() => {});
  ctx.state.conn = null;
  // The owner's own data protocol: Q08b leaves a protocol-6 (v1bump) owner, which refuses a
  // protocol-5 handshake (run-int2-all Q09), so its sessions were never closed and it never retired.
  const version = owner.hello?.protocol_version ?? 5;
  let dataError = null;
  const conn = await dataConn(ctx, ctx.layout.runtime, { version }).catch((error) => { dataError = String(error?.message ?? error); return null; });
  if (conn) {
    const desc = await conn.describe().catch(() => ({ sessions: [] }));
    for (const s of desc.sessions ?? []) await conn.call({ type: 'close', sessionId: s.backendId }).catch(() => {});
    await conn.close().catch(() => {});
  }
  const ctl = await controlConn(ctx);
  if (!ctl) {
    // An owner already retiring (a GUI sent RetireWhenIdle) exits once the sessions above close and
    // takes its endpoint with it (only3 Q10: owner 23480 exited at 07:38:59 with no endpoint left).
    const gone = await ctx.agent.call('waitExit', { pid: owner.pid, deadlineMs: 10_000 });
    return { retired: !gone.timedOut, pid: owner.pid, reason: 'no control endpoint', exited: gone, dataError };
  }
  const exit = ctx.agent.call('waitExit', { pid: owner.pid, deadlineMs: 30_000 });
  // An owner with zero sessions retires at once and can drop the control connection before its
  // reply is read (only2 Q09: the v1bump owner, drained over protocol 6, gave ECONNRESET). A lost
  // reply counts only when the owner's exit is then observed.
  let replyError = null;
  const resp = await ctl.retireWhenIdle().catch((error) => { replyError = String(error?.message ?? error); return null; });
  const exited = await exit;
  return { retired: (resp?.ok === true || replyError !== null) && !exited.timedOut, pid: owner.pid, resp, replyError, exited, dataError };
}

/** Launches a GUI, waits for a new owner and seeds one local session; records the daemon pid. */
export async function freshOwner(ctx, artifact, { env = {}, tag = 'QA_A', dataVersion = 5, expectKey = null } = {}) {
  const before = await ctx.agent.call('readFile', { file: hjoin(ctx.layout.runtime, 'owner.json') });
  const fromPid = before ? JSON.parse(before).pid : null;
  const gui = await ctx.gui.launch(artifact, { env });
  ctx.state.gui = gui;
  const changed = await ctx.agent.call('waitOwnerChange', { runtimeDir: ctx.layout.runtime, fromPid, deadlineMs: DAEMON_READY_TIMEOUT_MS * 2 });
  if (!changed.owner) throw new Error(`no new owner after launching ${artifact.name}`);
  const identity = await ownerIdentity(ctx);
  // Seed only a confirmed owner; a published record whose pid already died is OWNER_GONE.
  const confirmed = confirmOwner(identity);
  if (!confirmed.ok) throw new OwnerStateError(confirmed.code, { ...confirmed.detail, artifact: artifact.name, seen: changed.seen });
  // run-9a5db442 Q13: an enabled generation unit left by an earlier scenario won the lock and
  // became the "fresh" owner. With expectKey only the launched artifact's generation is accepted.
  if (expectKey && identity.hello?.generation_key !== expectKey) {
    throw new OwnerStateError('OWNER_WRONG_GENERATION', { pid: identity.pid, expected: expectKey, actual: identity.hello?.generation_key ?? null, artifact: artifact.name, seen: changed.seen });
  }
  await ctx.agent.call('record', { pid: identity.pid, role: `daemon-${artifact.name}`, executable: identity.os.executable });
  ctx.state.daemon = identity;
  ctx.state.conn = await dataConn(ctx, ctx.layout.runtime, { version: dataVersion });
  ctx.state.localA = await seedLocalSession(ctx, ctx.state.conn, tag);
  return identity;
}

/**
 * After a scenario threw: end its GUI (registry kill of the recorded pid), close every session the
 * owner still has through a fresh connection at the owner's own protocol, retire that owner
 * (RetireWhenIdle) and start a fresh owner from `artifact`, so a leftover owner of the failed
 * scenario (Q08b's v1bump owner in run-9a5db442) never becomes the next scenario's owner.
 * Returns { ok, ... }; it never throws.
 */
export async function recoverOwnerAfterException(ctx, artifact) {
  const out = { ok: false, gui: null, closed: [], retired: null, killed: null, fresh: null };
  try {
    if (ctx.state.gui) {
      out.gui = await ctx.gui.kill(ctx.state.gui).catch((e) => ({ error: String(e?.message ?? e) }));
      ctx.state.gui = null;
    }
    await invalidateOwnerState(ctx);
    const owner = await ownerIdentity(ctx);
    if (owner.pid && owner.os) {
      const version = owner.hello?.protocol_version ?? 5;
      const opened = await ctx.agent.call('dataOpen', { runtimeDir: ctx.layout.runtime, version, allowMismatch: true }).catch(() => null);
      if (opened?.handshake?.type === 'handshakeOk') {
        // ListSessionsOk.sessions is Vec<String> (protocol.rs ListSessionsOk): bare backend ids.
        // run-r5c read s.sessionId, sent close(undefined) and closed nothing.
        const listed = await ctx.agent.call('call', { handle: opened.handle, frame: D.listSessions() }).catch(() => null);
        for (const id of listed?.sessions ?? []) {
          const resp = await ctx.agent.call('call', { handle: opened.handle, frame: D.close(id) }).catch((e) => ({ error: String(e?.message ?? e) }));
          out.closed.push({ id, resp });
        }
      }
      if (opened) await ctx.agent.call('close', { handle: opened.handle }).catch(() => {});
    }
    out.retired = await retireOwner(ctx);
    if (!out.retired.retired && out.retired.pid && owner.pid === out.retired.pid) {
      // The owner still reports live sessions past its retire deadline. It is this run's QA owner,
      // recorded at start, so end it by that recorded pid (the registry refuses any other pid).
      out.killed = await killRecordedOwner(ctx, out.retired.pid);
      if (out.killed.exited) out.retired = { ...(await retireOwner(ctx)), after: 'killed by recorded pid' };
    }
    if (!out.retired.retired) return { ...out, code: 'OWNER_STATE_UNRECOVERABLE' };
    out.fresh = (await freshOwner(ctx, artifact)).pid;
    out.ok = true;
    return out;
  } catch (error) {
    return { ...out, code: error?.code ?? null, error: String(error?.message ?? error) };
  }
}

/** SIGTERM, then SIGKILL, a recorded QA owner pid; each step bounded by waitExit. */
export async function killRecordedOwner(ctx, pid) {
  const steps = [];
  for (const signal of ['SIGTERM', 'SIGKILL']) {
    let sent;
    try { sent = await ctx.agent.call('kill', { pid, signal }); } catch (e) { steps.push({ signal, refused: e?.code ?? null, error: String(e?.message ?? e) }); return { pid, exited: false, steps }; }
    const exit = await ctx.agent.call('waitExit', { pid, deadlineMs: 10_000 });
    steps.push({ signal, sent, exit });
    if (!exit?.timedOut) return { pid, exited: true, steps };
  }
  return { pid, exited: false, steps };
}
