// Private sshd fixture for Q08/Q11: 127.0.0.1, QA-dir host key, authorized_keys and port, recorded pid.
// Also writes the daemon's SSH inventory (<FERRYX_DATA_DIR>/ssh_hosts.json + remote_projects.json)
// so a spawn with startup { kind: remoteSsh } resolves to this sshd and nothing else.

import { createHash } from 'node:crypto';
import { hjoin } from './hostpath.mjs';
import { sshKnownHostsFile } from './guards.mjs';

export const SSHD_BINARIES = Object.freeze({
  mac: '/usr/sbin/sshd',
  linux: '/usr/sbin/sshd',
  windows: 'C:\\Windows\\System32\\OpenSSH\\sshd.exe',
});

/**
 * sshd_config for the private fixture. Win32-OpenSSH has no PAM and exits on "Unsupported option
 * UsePAM" (measured: run-q00fix-all/runlog-sshd.log), so the option is written only off Windows.
 */
export function sshdConfig({ port, hostKey, authorizedKeys, pidFile, user, platform = 'linux' }) {
  return [
    `Port ${port}`,
    'ListenAddress 127.0.0.1',
    `HostKey ${hostKey}`,
    `AuthorizedKeysFile ${authorizedKeys}`,
    `PidFile ${pidFile}`,
    'PasswordAuthentication no',
    'KbdInteractiveAuthentication no',
    'PubkeyAuthentication yes',
    ...(platform === 'windows' ? [] : ['UsePAM no']),
    'StrictModes no',
    `AllowUsers ${user}`,
    'LogLevel VERBOSE',
    '',
  ].join('\n');
}

/**
 * sshd arguments. Windows logs to a file (-E), never to stderr (-e): Win32-OpenSSH sshd -D with -e
 * and its stderr on a pipe accepts TCP but never sends its banner (measured on maho-win, run-r6d
 * q11-probe/banner-timing*.json: -e with pipes 3/3 timed out at 12 s; -E with the same pipes 3/3
 * banners in <20 ms), which is Q11's "Connection timed out during banner exchange".
 */
export function sshdArgs({ config, logFile, platform }) {
  return platform === 'windows' ? ['-D', '-E', logFile, '-f', config] : ['-D', '-e', '-f', config];
}

/**
 * Windows readiness: sshd -E holds sshd.log exclusively while it runs (EBUSY, run-r6d
 * q11-probe/sshd-E-log-lock.json), so the wait reads the SSH banner on the port instead - the exact
 * thing Q11's ssh needs. Retries the connect every 200 ms up to deadlineMs; prints the banner line.
 */
export const BANNER_PROBE_SCRIPT = "const net=require('net');const [port,ms]=process.argv.slice(1).map(Number);const end=Date.now()+ms;const go=()=>{const s=net.createConnection({host:'127.0.0.1',port});let g='';const retry=()=>{s.destroy();if(Date.now()<end)setTimeout(go,200);else process.exit(1);};s.setTimeout(2000,retry);s.on('data',(b)=>{g+=b;if(g.includes('\\n')){console.log(g.split('\\n')[0].trim());process.exit(0);}});s.on('error',retry);};go();";

async function waitListening(ctx, { logFile, port, deadlineMs }) {
  if (ctx.platform !== 'windows') return ctx.agent.call('waitLog', { file: logFile, pattern: `Server listening on 127\\.0\\.0\\.1 port ${port}`, deadlineMs });
  const r = await ctx.agent.call('exec', { command: 'node', args: ['-e', BANNER_PROBE_SCRIPT, String(port), String(deadlineMs)], deadlineMs: deadlineMs + 5_000 });
  const banner = String(r.stdout ?? '').trim();
  return r.status === 0 && banner.startsWith('SSH-') ? { matched: banner } : { matched: null, timedOut: true };
}

/** projects.rs identity(): ssh:<sha256(len(host_id) as u64 LE || host_id || root)>. */
export function remoteWorkspaceId(hostId, root) {
  const len = Buffer.alloc(8);
  len.writeBigUInt64LE(BigInt(Buffer.byteLength(hostId)));
  const hash = createHash('sha256').update(len).update(hostId).update(root).digest('hex');
  return `ssh:${hash}`;
}

export function sshInventory({ hostId, port, user, identityFile, repoRoot, platform = 'linux' }) {
  const workspaceId = remoteWorkspaceId(hostId, repoRoot);
  const hosts = { hosts: [{ id: hostId, label: 'QA private sshd', hostname: '127.0.0.1', username: user, port, identityFile, source: 'manual', authMethod: 'key' }], tombstones: [] };
  // projects.rs resolve() validates repoRoot against the record's platform and defaults to posix
  // (projects.rs:285-288), so a Windows root (C:\...) must carry platform "windows" (run-r6c Q11).
  const projects = { [workspaceId]: { workspaceId, hostId, repoRoot, gitRoot: repoRoot, ...(platform === 'windows' ? { platform: 'windows' } : {}) } };
  return { workspaceId, hosts, projects };
}

/** helper_setup.rs host_digest(): the first 32 hex chars of sha256(host_id). */
export function sshHelperDigest(hostId) {
  return createHash('sha256').update(hostId).digest('hex').slice(0, 32);
}

/**
 * The remote-helper daemons the private sshd's SSH sessions start for hostId (helper_setup.rs
 * qualified_location: <home>/.ferryx/versions/<tag>/bin/ferryx-remote-helper[.exe] running
 * `daemon --root <home>/.ferryx/r/<tag>/<digest(hostId)> --host-id <hostId>`). A row matches only
 * when its executable, root and host id are exactly those paths; returns [{ pid, executable, root }].
 */
export function findSshHelpers(rows, { platform, home, hostId }) {
  const win = platform === 'windows';
  const sep = win ? '\\' : '/';
  const base = home.replace(win ? /[\\/]+$/ : /\/+$/, '');
  const rootBase = [base, '.ferryx', 'r', ''].join(sep);
  const digest = sshHelperDigest(hostId);
  const same = (a, b) => (win ? a.toLowerCase() === b.toLowerCase() : a === b);
  const out = [];
  for (const row of rows) {
    const m = /(?:^|\s)"?daemon"?\s+"?--root"?\s+"?([^"]+?)"?\s+"?--host-id"?\s+"?([^"\s]+)"?\s*$/.exec(row.commandLine ?? '');
    if (!m || m[2] !== hostId || !row.executable) continue;
    // Windows helpers get their root as a verbatim path (`\\?\C:\...`, run-r6d q11-only helper 4040).
    const root = win ? m[1].replace(/^\\\\\?\\/, '') : m[1];
    if (!same(root.slice(0, rootBase.length), rootBase)) continue;
    const rest = root.slice(rootBase.length).split(sep);
    if (rest.length !== 2 || !rest[0] || rest[0].includes('/') || rest[1] !== digest) continue;
    const exe = [base, '.ferryx', 'versions', rest[0], 'bin', win ? 'ferryx-remote-helper.exe' : 'ferryx-remote-helper'].join(sep);
    if (!same(row.executable, exe)) continue;
    out.push({ pid: row.pid, executable: row.executable, root });
  }
  return out;
}

/** Records this run's SSH remote-helper daemon pids and roots for teardown (agent recordSshHelper). */
export async function recordSshHelper(ctx) {
  return ctx.agent.call('recordSshHelper', { hostId: ctx.sshd.hostId });
}

/** Starts sshd under the agent and records its pid. Returns null + reason when unavailable. */
export async function startSshd(ctx) {
  const bin = SSHD_BINARIES[ctx.platform];
  if (!(await ctx.agent.call('exists', { file: bin }))) return { unavailable: `sshd not found at ${bin}` };
  const dir = hjoin(ctx.layout.root, 'sshd');
  await ctx.agent.call('mkdir', { dir, mode: 0o700 });
  const hostKey = hjoin(dir, 'host_ed25519');
  const clientKey = hjoin(dir, 'client_ed25519');
  for (const key of [hostKey, clientKey]) {
    // A previous attempt in this run (e.g. Q08 failing after keygen) leaves the key files; ssh-keygen
    // then prompts to overwrite on a closed stdin and fails with empty stderr. Start from no keys.
    await ctx.agent.call('remove', { target: key });
    await ctx.agent.call('remove', { target: `${key}.pub` });
    const r = await ctx.agent.call('exec', { command: 'ssh-keygen', args: ['-q', '-t', 'ed25519', '-N', '', '-f', key] });
    if (r.status !== 0) return { unavailable: `ssh-keygen failed: ${r.stderr}` };
  }
  const pub = await ctx.agent.call('readFile', { file: `${clientKey}.pub` });
  const authorizedKeys = hjoin(dir, 'authorized_keys');
  await ctx.agent.call('writeFile', { file: authorizedKeys, content: pub, mode: 0o600 });
  const user = ctx.hostUser;
  const port = ctx.sshPort;
  const pidFile = hjoin(dir, 'sshd.pid');
  const config = hjoin(dir, 'sshd_config');
  await ctx.agent.call('writeFile', { file: config, content: sshdConfig({ port, hostKey, authorizedKeys, pidFile, user, platform: ctx.platform }) });
  // Config test first: every unsupported option is reported in one go, never as a silent exit.
  const checked = await ctx.agent.call('exec', { command: bin, args: ['-t', '-f', config] });
  if (checked.status !== 0) return { unavailable: `sshd -t rejected the config: ${String(checked.stderr || checked.stdout).trim()}` };
  const logFile = hjoin(ctx.layout.logs, 'sshd.log');
  // With -E sshd owns sshd.log; the agent's stdout/stderr capture goes beside it, not into it.
  const captureFile = ctx.platform === 'windows' ? hjoin(ctx.layout.logs, 'sshd.stdio.log') : logFile;
  const entry = await ctx.agent.call('spawn', { command: bin, args: sshdArgs({ config, logFile, platform: ctx.platform }), role: 'sshd', executable: bin, logFile: captureFile });
  // Listening, or the recorded sshd's own exit, whichever comes first.
  const exited = ctx.agent.call('waitExit', { pid: entry.pid, deadlineMs: 15_000 });
  const outcome = await Promise.race([
    waitListening(ctx, { logFile, port, deadlineMs: 15_000 }).then((l) => ({ listening: l })),
    exited.then((x) => (x?.timedOut ? new Promise(() => {}) : { exited: x })),
  ]);
  if (outcome.exited) return { unavailable: `sshd exited before listening: ${String((await ctx.agent.call('readFile', { file: logFile })) ?? '').trim().slice(-2000)}`, pid: entry.pid };
  if (!outcome.listening.matched) return { unavailable: 'sshd did not report listening', pid: entry.pid };
  const hostPub = (await ctx.agent.call('readFile', { file: `${hostKey}.pub` })).trim().split(' ').slice(0, 2).join(' ');
  // The same file isolationEnv hands the QA daemon as FERRYX_QA_SSH_KNOWN_HOSTS.
  const knownHosts = sshKnownHostsFile(ctx.layout);
  await ctx.agent.call('writeFile', { file: knownHosts, content: `[127.0.0.1]:${port} ${hostPub}\n` });
  const repoRoot = hjoin(ctx.layout.root, 'ssh-repo');
  await ctx.agent.call('mkdir', { dir: repoRoot });
  await ctx.agent.call('exec', { command: 'git', args: ['init', '-q', repoRoot] });
  const hostId = sshHostId(ctx.runId);
  const inv = sshInventory({ hostId, port, user, identityFile: clientKey, repoRoot, platform: ctx.platform });
  await ctx.agent.call('writeFile', { file: hjoin(ctx.layout.data, 'ssh_hosts.json'), content: JSON.stringify(inv.hosts) });
  await ctx.agent.call('writeFile', { file: hjoin(ctx.layout.data, 'remote_projects.json'), content: JSON.stringify(inv.projects) });
  return { pid: entry.pid, port, user, clientKey, knownHosts, hostId, workspaceId: inv.workspaceId, hostStorePath: hjoin(ctx.layout.data, 'ssh_hosts.json'), repoRoot };
}

/**
 * The GUI's own restore path onto an existing SSH backend: a saved workspace session whose SSH
 * workspace (target ssh/hostId) holds one tab bound to backendSessionId. App.tsx recovers the
 * project from it (recoverProjectBootstrap) and the restored pane re-admits and attaches that
 * session (sessionPersistence.ts keeps an SSH backendSessionId). Fields outside workspaces (the
 * daemon's durable remote records) are kept from the existing file.
 */
export function sshRestoreSession(existing, { workspaceId, hostId, repoRoot, backendSessionId, daemonEpoch, incarnation, localSessionId, tabId, now }) {
  const base = existing && typeof existing === 'object' ? existing : {};
  return {
    ...base,
    version: 2,
    timestamp: now,
    activeWorkspaceId: workspaceId,
    workspaces: {
      [workspaceId]: {
        workspaceId, repoRoot, gitRoot: repoRoot, target: { kind: 'ssh', hostId },
        worktrees: [{ path: repoRoot, branch: '', head: '', isMain: true, isLocked: false }],
        activeWorktreePath: repoRoot,
        layout: { tabs: [{ id: tabId, label: 'shell', sessionId: localSessionId }], activeTabId: tabId, splitMode: 'none', primaryTabId: tabId, secondaryTabId: null },
        terminalSessions: {
          [localSessionId]: { sessionId: localSessionId, localSessionId, backendSessionId, cwd: repoRoot, worktreePath: repoRoot, createdAt: now, daemonEpoch, incarnation },
        },
      },
    },
  };
}

/** The private sshd's host id: unique per run, so its helper is this run's own. */
export function sshHostId(runId) {
  return `qa-sshd-${runId}`;
}
