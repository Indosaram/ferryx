import { describe, expect, test } from 'bun:test';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { mkdtempSync, mkdirSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { guiIdentity } from './desktop-lane.mjs';
import { activateGui, guiBounds } from './host-helper.mjs';

describe('N3.1 desktop lane GUI arguments', () => {
  test('guiIdentity reads the recorded candidate checkout, not the host worktree', () => {
    const candidate = '/run/checkouts/candidate';
    const lane = { receipt: { candidate: { checkout: candidate }, harness: { checkout: '/run/harness' } } };
    expect(guiIdentity(lane, { pid: 7, executable: '/run/checkouts/candidate/gui' })).toEqual({
      pid: 7, executable: '/run/checkouts/candidate/gui', checkout: candidate,
    });
  });

  test('guiIdentity carries the exact pid and executable it was given', () => {
    const lane = { receipt: { candidate: { checkout: '/c' } } };
    expect(guiIdentity(lane, { pid: 4212, executable: '/c/ferryx.exe' })).toEqual({ pid: 4212, executable: '/c/ferryx.exe', checkout: '/c' });
  });
});

test('desktop lane calls the host helper only through guiIdentity', () => {
  // N3.1: the call-site wiring in desktop-lane.mjs is what actually reaches the host helper, so
  // it is asserted directly: both GUI calls must pass the extracted identity and no call may
  // rebuild the arguments inline.
  const source = readFileSync(new URL('./desktop-lane.mjs', import.meta.url), 'utf8');
  const guiCalls = source.split('\n').filter((line) => /helper\.call\('(activate-gui|gui-bounds)'/.test(line));
  expect(guiCalls).toHaveLength(2);
  for (const line of guiCalls) expect(line).toContain('guiIdentity(lane, gui)');
  expect(source).not.toMatch(/helper\.call\('(?:activate-gui|gui-bounds)',\s*\{/);
});

describe('F6-4 recorded GUI checkout', () => {
  for (const [method, code] of [[activateGui, 'ACTIVATE_REFUSED'], [guiBounds, 'BOUNDS_REFUSED']]) {
    for (const variant of ['candidate', 'system', 'sibling prefix', 'symlink outside', 'unrecorded checkout', 'ancestor checkout', 'missing checkout', 'relative checkout', 'relative executable', 'symlink checkout']) {
      test(`${method.name} ${variant}`, async () => {
        const root = realpathSync(mkdtempSync(join(tmpdir(), 'f6-gui-')));
        try {
          const candidate = join(root, 'checkout');
          const other = join(root, 'unrecorded');
          const evil = `${candidate}-evil`;
          const harness = realpathSync(join(dirname(fileURLToPath(import.meta.url)), '../../..'));
          expect(candidate).not.toBe(harness);
          for (const dir of [candidate, other, evil]) {
            mkdirSync(dir);
            writeFileSync(join(dir, 'gui'), 'fixture executable');
          }
          // N1: a candidate checkout is the top level of its own git work tree, so the fixture
          // must be one; a plain directory would make even the positive case refuse.
          execFileSync('git', ['-C', candidate, 'init', '--quiet']);
          symlinkSync(join(other, 'gui'), join(candidate, 'escape'));
          symlinkSync(candidate, join(root, 'alias'));
          const lane = { receipt: { candidate: { checkout: candidate }, harness: { checkout: harness } } };
          const args = { pid: 123, executable: join(candidate, 'gui'), checkout: lane.receipt.candidate.checkout };
          if (variant === 'system') args.executable = '/usr/bin/true';
          if (variant === 'sibling prefix') args.executable = join(evil, 'gui');
          if (variant === 'symlink outside') args.executable = join(candidate, 'escape');
          if (variant === 'unrecorded checkout') args.executable = join(other, 'gui');
          // N1: an ancestor such as the temp root (or `/`) contains the real binary, so without
          // a requirement that the checkout be its own git top level the binary would be accepted.
          if (variant === 'ancestor checkout') args.checkout = root;
          if (variant === 'missing checkout') delete args.checkout;
          if (variant === 'relative checkout') args.checkout = 'checkout';
          if (variant === 'relative executable') args.executable = 'gui';
          if (variant === 'symlink checkout') args.checkout = join(root, 'alias');
          const calls = [];
          const operation = method(args, { mac: true, invoke: async (...call) => { calls.push(call); return '{"fixture":true}'; } });
          if (variant === 'candidate') {
            expect(await operation).toEqual({ fixture: true });
            expect(calls).toHaveLength(1);
            expect(calls[0][0]).toBe('/usr/bin/osascript');
            expect(calls[0][1].at(-1)).toContain(args.executable);
          } else {
            await expect(operation).rejects.toMatchObject({ code });
            expect(calls).toEqual([]);
          }
        } finally { rmSync(root, { recursive: true, force: true }); }
      });
    }
  }
});
