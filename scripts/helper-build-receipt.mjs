// Run on the artifact's build/execution host after an explicit locked build.
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import { computeHelperSourceFingerprint, computeSha256 } from './build-remote-helpers.mjs';

const [target, executableArg, outputArg] = process.argv.slice(2);
assert(target && executableArg && outputArg, 'target executable output.json required');
const executable = resolve(executableArg);
// Execute, never infer capability evidence from strings embedded in the image.
const report = JSON.parse(execFileSync(executable, ['--capabilities'], { encoding: 'utf8', timeout: 30_000 }));
assert.equal(report.protocol, 1);
const { fingerprint: sourceFingerprint, sources } = computeHelperSourceFingerprint(process.cwd());
writeFileSync(resolve(outputArg), JSON.stringify({
  target, profile: 'release', executable,
  sha256: computeSha256(readFileSync(executable)),
  helperVersion: report.helperVersion, capabilities: report.capabilities,
  sourceFingerprint, sources,
}, null, 2) + '\n');
