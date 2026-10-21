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

export function sshdConfig({ port, hostKey, authorizedKeys, pidFile, user }) {
  return [
    `Port ${port}`,
    'ListenAddress 127.0.0.1',
    `HostKey ${hostKey}`,
    `AuthorizedKeysFile ${authorizedKeys}`,
    `PidFile ${pidFile}`,
    'PasswordAuthentication no',
    'KbdInteractiveAuthentication no',
    'PubkeyAuthentication yes',
    'UsePAM no',
    'StrictModes no',
    `AllowUsers ${user}`,
    'LogLevel VERBOSE',
    '',
  ].join('\n');
}

/** projects.rs identity(): ssh:<sha256(len(host_id) as u64 LE || host_id || root)>. */
export function remoteWorkspaceId(hostId, root) {
  const len = Buffer.alloc(8);
  len.writeBigUInt64LE(BigInt(Buffer.byteLength(hostId)));
  const hash = createHash('sha256').update(len).update(hostId).update(root).digest('hex');
  return `ssh:${hash}`;
}

export function sshInventory({ hostId, port, user, identityFile, repoRoot }) {
  const workspaceId = remoteWorkspaceId(hostId, repoRoot);
  const hosts = { hosts: [{ id: hostId, label: 'QA private sshd', hostname: '127.0.0.1', username: user, port, identityFile, source: 'manual', authMethod: 'key' }], tombstones: [] };
  const projects = { [workspaceId]: { workspaceId, hostId, repoRoot, gitRoot: repoRoot } };
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
    const root = m[1];
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
  await ctx.agent.call('writeFile', { file: config, content: sshdConfig({ port, hostKey, authorizedKeys, pidFile, user }) });
  const logFile = hjoin(ctx.layout.logs, 'sshd.log');
  const entry = await ctx.agent.call('spawn', { command: bin, args: ['-D', '-e', '-f', config], role: 'sshd', executable: bin, logFile });
  const listening = await ctx.agent.call('waitLog', { file: logFile, pattern: `Server listening on 127\\.0\\.0\\.1 port ${port}`, deadlineMs: 15_000 });
  if (!listening.matched) return { unavailable: 'sshd did not report listening', pid: entry.pid };
  const hostPub = (await ctx.agent.call('readFile', { file: `${hostKey}.pub` })).trim().split(' ').slice(0, 2).join(' ');
  // The same file isolationEnv hands the QA daemon as FERRYX_QA_SSH_KNOWN_HOSTS.
  const knownHosts = sshKnownHostsFile(ctx.layout);
  await ctx.agent.call('writeFile', { file: knownHosts, content: `[127.0.0.1]:${port} ${hostPub}\n` });
  const repoRoot = hjoin(ctx.layout.root, 'ssh-repo');
  await ctx.agent.call('mkdir', { dir: repoRoot });
  await ctx.agent.call('exec', { command: 'git', args: ['init', '-q', repoRoot] });
  const hostId = sshHostId(ctx.runId);
  const inv = sshInventory({ hostId, port, user, identityFile: clientKey, repoRoot });
  await ctx.agent.call('writeFile', { file: hjoin(ctx.layout.data, 'ssh_hosts.json'), content: JSON.stringify(inv.hosts) });
  await ctx.agent.call('writeFile', { file: hjoin(ctx.layout.data, 'remote_projects.json'), content: JSON.stringify(inv.projects) });
  return { pid: entry.pid, hostId, port, user, clientKey, knownHosts, workspaceId: inv.workspaceId, hostStorePath: hjoin(ctx.layout.data, 'ssh_hosts.json'), repoRoot };
}

/** The private sshd's host id: unique per run, so its helper is this run's own. */
export function sshHostId(runId) {
  return `qa-sshd-${runId}`;
}
