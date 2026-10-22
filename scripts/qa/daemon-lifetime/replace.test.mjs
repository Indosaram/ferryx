import test from 'node:test';
import assert from 'node:assert/strict';
import { createAppReplacer } from './lib/replace.mjs';

/**
 * A host whose copy commands behave like the real ones: `cp -f x x` and PowerShell
 * `Copy-Item x x` fail on a self-copy, ditto copies a bundle dir. sha256 returns the file bytes.
 */
function fakeHost(platform, files) {
  const copyTree = (src, dst) => {
    for (const [p, v] of [...files]) if (p === src || p.startsWith(`${src}/`)) files.set(dst + p.slice(src.length), v);
  };
  const exec = ({ command, args }) => {
    if (command === 'cp') {
      const [, src, dst] = args;
      if (src === dst || !files.has(src)) return { status: 1, stdout: '', stderr: `cp: '${src}' and '${dst}' are the same file` };
      files.set(dst, files.get(src));
      return { status: 0, stdout: '', stderr: '' };
    }
    if (command === 'powershell.exe') {
      const m = /-LiteralPath '([^']+)' -Destination '([^']+)'/.exec(args.at(-1));
      if (!m || m[1] === m[2] || !files.has(m[1])) return { status: 1, stdout: '', stderr: 'Copy-Item : Cannot overwrite the item with itself.' };
      files.set(m[2], files.get(m[1]));
      return { status: 0, stdout: '', stderr: '' };
    }
    if (command === 'rm') { for (const p of [...files.keys()]) if (p === args[1] || p.startsWith(`${args[1]}/`)) files.delete(p); return { status: 0 }; }
    if (command === 'ditto') { if (args[0] === args[1]) return { status: 1 }; copyTree(args[0], args[1]); return { status: 0 }; }
    throw new Error(`unexpected ${command}`);
  };
  return { call: async (method, params) => (method === 'exec' ? exec(params) : method === 'sha256' ? files.get(params.file) : method === 'mkdir' ? true : null) };
}

const LAYOUTS = {
  linux: { root: '/run/artifacts', art: (n) => ({ name: n, appPath: null, guiExecutable: `/run/artifacts/${n}/ferryx` }) },
  windows: { root: 'C:\\run\\artifacts', art: (n) => ({ name: n, appPath: null, guiExecutable: `C:\\run\\artifacts\\${n}\\ferryx.exe` }) },
  mac: { root: '/run/artifacts', art: (n) => ({ name: n, appPath: `/run/artifacts/${n}/Ferryx QA.app`, guiExecutable: `/run/artifacts/${n}/Ferryx QA.app/Contents/MacOS/ferryx` }) },
};

for (const [platform, layout] of Object.entries(LAYOUTS)) {
  test(`${platform}: Q07 replace A with B, then put A back, leaves A's own bytes at A's path`, async () => {
    const A = layout.art('A');
    const B = layout.art('B');
    const files = new Map([[A.guiExecutable, 'bytes-A'], [B.guiExecutable, 'bytes-B']]);
    const replace = createAppReplacer({ platform, agent: fakeHost(platform, files), pristineRoot: `${layout.root}${platform === 'windows' ? '\\' : '/'}pristine` });
    // Q07's exact call sequence (q07-q09.mjs): replace(A, B), then replace(replaced.artifact, A).
    const replaced = await replace(A, B);
    assert.equal(files.get(A.guiExecutable), 'bytes-B');
    assert.equal(replaced.artifact.guiExecutable, A.guiExecutable, 'the replaced artifact keeps A\'s path');
    assert.equal(replaced.artifact.sha256, 'bytes-B');
    const restored = await replace(replaced.artifact, A);
    assert.equal(files.get(A.guiExecutable), 'bytes-A', 'A is restored');
    assert.equal(restored.artifact.sha256, 'bytes-A');
    assert.equal(files.get(B.guiExecutable), 'bytes-B', 'B is untouched');
    // A second round still has A's original to restore from.
    await replace(A, B);
    await replace(A, A);
    assert.equal(files.get(A.guiExecutable), 'bytes-A');
  });
}

test('a failed copy throws instead of reporting a replacement that did not happen', async () => {
  const A = LAYOUTS.linux.art('A');
  const B = LAYOUTS.linux.art('B');
  const files = new Map([[A.guiExecutable, 'bytes-A']]); // B missing: cp fails
  const replace = createAppReplacer({ platform: 'linux', agent: fakeHost('linux', files), pristineRoot: '/run/artifacts/pristine' });
  await assert.rejects(replace(A, B), (e) => e.code === 'REPLACE_COPY_FAILED');
  assert.equal(files.get(A.guiExecutable), 'bytes-A');
});

/** Windows host whose processes keep their image locked until they exit (all2 Q07). */
function lockingHost(files, procs) {
  const base = fakeHost('windows', files);
  const order = [];
  return {
    order,
    call: async (method, params) => {
      if (method === 'table') return procs.filter((p) => p.alive).map(({ pid, executable }) => ({ pid, ppid: 1, executable, commandLine: '' }));
      if (method === 'waitExit') {
        const p = procs.find((x) => x.pid === params.pid);
        order.push(`wait:${params.pid}`);
        if (p.exitsAfterWait) { p.alive = false; return { pid: p.pid, exitedAt: 1 }; }
        return { timedOut: true };
      }
      if (method === 'exec') {
        const dst = /-Destination '([^']+)'/.exec(params.args.at(-1))?.[1];
        if (procs.some((p) => p.alive && p.executable.toLowerCase() === dst?.toLowerCase())) return { status: 1, stdout: '', stderr: 'Copy-Item : used by another process' };
        order.push(`copy:${dst}`);
      }
      return base.call(method, params);
    },
  };
}

test('windows: Q07 waits for the exit of a process still imaged from the target dir before copying (all2 Q07)', async () => {
  const A = LAYOUTS.windows.art('A');
  const B = LAYOUTS.windows.art('B');
  const files = new Map([[A.guiExecutable, 'bytes-A'], [B.guiExecutable, 'bytes-B']]);
  // The quit A GUI still runs for a moment; a B process elsewhere is not A's business.
  const procs = [
    { pid: 25304, executable: 'C:\\RUN\\artifacts\\A\\ferryx.exe', alive: true, exitsAfterWait: true },
    { pid: 900, executable: B.guiExecutable, alive: true, exitsAfterWait: false },
  ];
  const agent = lockingHost(files, procs);
  const replace = createAppReplacer({ platform: 'windows', agent, pristineRoot: 'C:\\run\\artifacts\\pristine' });
  const replaced = await replace(A, B);
  assert.equal(files.get(A.guiExecutable), 'bytes-B');
  assert.deepEqual(agent.order.filter((o) => o === 'wait:25304' || o === `copy:${A.guiExecutable}`), ['wait:25304', `copy:${A.guiExecutable}`]);
  assert.ok(!agent.order.includes('wait:900'), 'a process outside the target dir is never waited on');
  assert.deepEqual(replaced.waitedFor.map((w) => w.pid), [25304]);
});

test('windows: a process that keeps the target imaged past the bound fails REPLACE_TARGET_BUSY, nothing copied or killed', async () => {
  const A = LAYOUTS.windows.art('A');
  const B = LAYOUTS.windows.art('B');
  const files = new Map([[A.guiExecutable, 'bytes-A'], [B.guiExecutable, 'bytes-B']]);
  const procs = [{ pid: 25304, executable: A.guiExecutable, alive: true, exitsAfterWait: false }];
  const agent = lockingHost(files, procs);
  const replace = createAppReplacer({ platform: 'windows', agent, pristineRoot: 'C:\\run\\artifacts\\pristine' });
  await assert.rejects(replace(A, B), (e) => e.code === 'REPLACE_TARGET_BUSY' && e.detail.busy.map((b) => b.pid).join() === '25304');
  assert.equal(files.get(A.guiExecutable), 'bytes-A');
  assert.ok(procs[0].alive, 'the holder is waited on, never killed');
});
