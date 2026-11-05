import { describe, expect, test } from 'bun:test';
import { mkdtempSync, mkdirSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { activateGui, guiBounds } from './host-helper.mjs';

describe('F6-4 recorded GUI checkout', () => {
  for (const [method, code] of [[activateGui, 'ACTIVATE_REFUSED'], [guiBounds, 'BOUNDS_REFUSED']]) {
    for (const variant of ['candidate', 'system', 'sibling prefix', 'symlink outside', 'unrecorded checkout', 'missing checkout', 'relative checkout', 'relative executable', 'symlink checkout']) {
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
          symlinkSync(join(other, 'gui'), join(candidate, 'escape'));
          symlinkSync(candidate, join(root, 'alias'));
          const lane = { receipt: { candidate: { checkout: candidate }, harness: { checkout: harness } } };
          const args = { pid: 123, executable: join(candidate, 'gui'), checkout: lane.receipt.candidate.checkout };
          if (variant === 'system') args.executable = '/usr/bin/true';
          if (variant === 'sibling prefix') args.executable = join(evil, 'gui');
          if (variant === 'symlink outside') args.executable = join(candidate, 'escape');
          if (variant === 'unrecorded checkout') args.executable = join(other, 'gui');
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
