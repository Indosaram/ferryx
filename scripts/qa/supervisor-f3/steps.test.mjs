// An artifact receipt must describe the file the run actually leaves behind.
//
// The OCR poll in desktop-lane.mjs retries by calling artifact('ocr', 'typed-token-line.ocr.tsv')
// once per attempt. lane.write() overwrites the same file each time, so the receipt recorded last
// is the only one whose bytes and sha256 still exist on disk. verify.mjs compares every receipt it
// is given against that one file, so a receipt per attempt can never all match: the run fails with
// "bytes/sha256 differ from the receipt" no matter how correct the screenshot was.
import { describe, expect, test } from 'bun:test';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { newLane, runStep } from './steps.mjs';

// The evidence dir must outlive the assertions, so cleanup is the test's own finally.
async function inLane(body) {
  const dir = mkdtempSync(join(tmpdir(), 'f3-steps-'));
  try {
    return await body(newLane('windows', dir), dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const matchesDisk = (dir, entry) => {
  const bytes = readFileSync(join(dir, entry.path));
  return bytes.length === entry.bytes && createHash('sha256').update(bytes).digest('hex') === entry.sha256;
};

describe('runStep artifact receipts', () => {
  test('a retried artifact leaves one receipt, and it matches the final bytes', async () => {
    let attempts = 0;
    await inLane(async (lane, dir) => {
      const step = await runStep(lane, 'F3-D6', async (ctx) => {
        while (attempts < 3) {
          attempts += 1;
          ctx.artifact('ocr', 'typed-token-line.ocr.tsv', `attempt ${attempts}\n`);
        }
      });
      expect(step.status).toBe('pass');
      const forPath = step.artifacts.filter((entry) => entry.path.endsWith('typed-token-line.ocr.tsv'));
      expect(forPath).toHaveLength(1);
      expect(readFileSync(join(dir, forPath[0].path), 'utf8')).toBe('attempt 3\n');
      expect(matchesDisk(dir, forPath[0])).toBe(true);
    });
  });

  test('every receipt in a retried step matches the file on disk', async () => {
    await inLane(async (lane, dir) => {
      const step = await runStep(lane, 'F3-D6', async (ctx) => {
        ctx.artifact('fixture-log', 'focus.agent.jsonl', 'first\n');
        ctx.artifact('fixture-log', 'focus.agent.jsonl', 'second\n');
        ctx.artifact('ocr', 'typed-token-line.ocr.tsv', 'tsv\n');
      });
      expect(step.artifacts.length).toBeGreaterThan(0);
      for (const entry of step.artifacts) expect(matchesDisk(dir, entry)).toBe(true);
    });
  });

  test('distinct artifact names each keep their own receipt', async () => {
    await inLane(async (lane) => {
      const step = await runStep(lane, 'F3-D6', async (ctx) => {
        ctx.artifact('png', 'a.png', 'a');
        ctx.artifact('png', 'b.png', 'b');
      });
      expect(step.artifacts.map((entry) => entry.path.split(/[\\/]/).pop())).toEqual(['a.png', 'b.png']);
    });
  });
});
