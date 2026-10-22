// Q07 app replacement on disk, portable to mac, linux and windows.
//
// Q07 overwrites artifact A's install path with B's bytes and later restores A. Restoring by
// copying "A onto A" is a self-copy: `cp -f x x` exits 1 ("are the same file") and
// `Copy-Item -Force x x` errors ("cannot overwrite the item with itself"), so A's path kept B's
// bytes and every later "A" GUI was B (run-q08: Q10 and Q13 OWNER_WRONG_GENERATION). Here the
// original bytes of every overwritten path are saved first, a restore reads that saved copy, a
// failed copy throws, and the result is checked by sha256.

export class ReplaceAppError extends Error {
  constructor(code, detail) {
    super(`${code}: ${JSON.stringify(detail)}`);
    this.code = code;
    this.detail = detail;
  }
}

const sep = (platform) => (platform === 'windows' ? '\\' : '/');
const join = (platform, ...parts) => parts.join(sep(platform));
const basename = (platform, p) => p.split(platform === 'windows' ? /[\\/]/ : /\//).pop();
const psQuote = (p) => `'${String(p).replace(/'/g, "''")}'`;
const dirname = (platform, p) => p.slice(0, p.lastIndexOf(sep(platform)));

/** Bound on the processes imaged under a Windows target dir exiting before it is overwritten. */
export const TARGET_FREE_DEADLINE_MS = 30_000;

/**
 * Windows keeps a running image locked: Copy-Item over it fails with a sharing violation (all2 Q07:
 * artifacts\A\ferryx.exe "used by another process" right after the A GUI was quit). Before the
 * copy, every process whose executable sits under the target's directory is waited on, bounded,
 * through its observed exit. Nothing is killed. Returns the holders seen; throws
 * REPLACE_TARGET_BUSY naming the ones still running at the deadline.
 */
async function waitTargetFree({ platform, agent }, target) {
  if (platform !== 'windows') return [];
  const dir = `${dirname(platform, target.guiExecutable)}\\`.toLowerCase();
  const holders = ((await agent.call('table')) ?? []).filter((r) => r.executable && r.executable.toLowerCase().startsWith(dir));
  const deadline = Date.now() + TARGET_FREE_DEADLINE_MS;
  const busy = [];
  for (const h of holders) {
    const exit = await agent.call('waitExit', { pid: h.pid, deadlineMs: Math.max(0, deadline - Date.now()) });
    if (exit?.timedOut) busy.push({ pid: h.pid, ppid: h.ppid, executable: h.executable, commandLine: h.commandLine });
  }
  if (busy.length) throw new ReplaceAppError('REPLACE_TARGET_BUSY', { target: target.guiExecutable, busy, deadlineMs: TARGET_FREE_DEADLINE_MS });
  return holders.map((h) => ({ pid: h.pid, executable: h.executable }));
}

/** One copy of an app (mac bundle dir, else the GUI executable); throws on a non-zero exit. */
async function copyApp({ platform, agent }, from, to) {
  const steps = platform === 'mac'
    ? [{ command: 'rm', args: ['-rf', to.appPath] }, { command: 'ditto', args: [from.appPath, to.appPath] }]
    : platform === 'windows'
      ? [{ command: 'powershell.exe', args: ['-NoProfile', '-Command', `$ErrorActionPreference='Stop'; Copy-Item -Force -LiteralPath ${psQuote(from.guiExecutable)} -Destination ${psQuote(to.guiExecutable)}`] }]
      : [{ command: 'cp', args: ['-f', from.guiExecutable, to.guiExecutable] }];
  for (const step of steps) {
    const r = await agent.call('exec', step);
    if (r?.status !== 0) throw new ReplaceAppError('REPLACE_COPY_FAILED', { step, status: r?.status ?? null, stderr: r?.stderr ?? null });
  }
}

/**
 * Returns replaceAppOnDisk(from, to): `from`'s install path gets `to`'s app. The first time a path
 * is overwritten its original app is saved under pristineRoot/<artifact name>; a later replace
 * whose `to` names that path (Q07 putting A back) copies from the saved original instead of from
 * the path itself.
 */
export function createAppReplacer({ platform, agent, pristineRoot }) {
  const saved = new Map(); // install path (guiExecutable) -> artifact paths of its saved original
  const pristineOf = async (art) => {
    if (saved.has(art.guiExecutable)) return saved.get(art.guiExecutable);
    const dir = join(platform, pristineRoot, art.name);
    await agent.call('mkdir', { dir });
    const copy = platform === 'mac'
      ? { appPath: join(platform, dir, basename(platform, art.appPath)), guiExecutable: join(platform, dir, basename(platform, art.appPath), art.guiExecutable.slice(art.appPath.length + 1)) }
      : { appPath: null, guiExecutable: join(platform, dir, basename(platform, art.guiExecutable)) };
    await copyApp({ platform, agent }, art, copy);
    saved.set(art.guiExecutable, copy);
    return copy;
  };
  return async (from, to) => {
    // Save the target's original before it is overwritten, then read the source from its saved
    // original when it has one (the target itself, on a restore).
    await pristineOf(from);
    const source = saved.get(to.guiExecutable) ?? to;
    if (source.guiExecutable === from.guiExecutable) throw new ReplaceAppError('REPLACE_SELF_COPY', { path: from.guiExecutable });
    const waitedFor = await waitTargetFree({ platform, agent }, from);
    await copyApp({ platform, agent }, source, from);
    const [got, want] = [await agent.call('sha256', { file: from.guiExecutable }), await agent.call('sha256', { file: source.guiExecutable })];
    if (got !== want) throw new ReplaceAppError('REPLACE_VERIFY_FAILED', { path: from.guiExecutable, got, want, source: source.guiExecutable });
    return { artifact: { ...from, name: to.name, sha256: got }, source: source.guiExecutable, waitedFor };
  };
}
