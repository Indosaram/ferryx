import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import vm from 'node:vm';
import { stageHelpers, computeSha256 } from './build-remote-helpers.mjs';

// Original stager's constants are redirected to an owned root without executing
// its shared-resource entry point. Final API uses the same isolated paths.
async function stage(root, options) {
  const source = readFileSync(new URL('./build-remote-helpers.mjs', import.meta.url), 'utf8');
  if (source.includes('stageHelpers({')) return stageHelpers({ repoRoot: root, resourcesDir: join(root, 'out'), ...options });
  const context = vm.createContext({ console });
  const module = new vm.SourceTextModule(source, { context, initializeImportMeta(meta) { meta.dirname = join(root, 'scripts'); meta.main = false; } });
  await module.link(async specifier => {
    const actual = await import(specifier);
    return new vm.SyntheticModule(Object.keys(actual), function() { for (const key of Object.keys(actual)) this.setExport(key, actual[key]); }, { context });
  });
  await module.evaluate();
  return module.namespace.stageHelpers(options);
}
const target = 'x86_64-pc-windows-msvc';
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'p25-stage-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'remote-helper'), { recursive: true });
  writeFileSync(join(root, 'remote-helper/Cargo.toml'), '[package]\nversion = "2026.908.1"\n');
  writeFileSync(join(root, 'remote-helper/Cargo.lock'), 'owned lock');
  return root;
}
for (const mode of ['empty', 'gnu', 'debug', 'stale']) test(`stager rejects ${mode} without publication`, async t => {
  const root = fixture(t);
  const rel = mode === 'gnu' ? 'remote-helper/target/x86_64-pc-windows-gnu/release' : mode === 'debug' ? `remote-helper/target/${target}/debug` : `src-tauri/resources/helpers/${target}`;
  if (mode !== 'empty') { mkdirSync(join(root, rel), { recursive: true }); writeFileSync(join(root, rel, 'ferryx-remote-helper.exe'), 'stale'); }
  await assert.rejects(stage(root, { requiredTargets: [target], receipts: [] }));
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
function receipt(root) {
  const path = join(root, 'helper.exe');
  // Minimal PE machine fixture; provenance is explicit, never inferred from name.
  const bytes = Buffer.alloc(256); bytes.write('MZ'); bytes.writeUInt32LE(128, 60); bytes.write('PE\0\0', 128); bytes.writeUInt16LE(0x8664, 132); writeFileSync(path, bytes);
  const sources = ['remote-helper/Cargo.toml', 'remote-helper/Cargo.lock'].map(path => ({ path, sha256: computeSha256(readFileSync(join(root, path))) }));
  return { target, profile: 'release', executable: path, sha256: computeSha256(bytes), sources };
}
test('stager requires full matrix before publishing any bytes', async t => {
  const root = fixture(t); const r = receipt(root);
  await assert.rejects(stage(root, { receipts: [r], requiredTargets: [target, 'aarch64-unknown-linux-gnu'] }));
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
test('stager binds explicit artifact bytes, source lock and helper protocol 1', async t => {
  const root = fixture(t); const r = receipt(root);
  const manifest = await stage(root, { receipts: [r], requiredTargets: [target] });
  assert.equal(manifest.protocolVersion, 1);
  assert.equal(manifest.artifacts.length, 1);
  assert.equal(manifest.artifacts[0].sha256, r.sha256);
  assert.deepEqual(readFileSync(join(root, 'out', target, 'ferryx-remote-helper.exe')), readFileSync(r.executable));
});
for (const mismatch of ['hash', 'lock', 'machine', 'duplicate']) test(`stager rejects ${mismatch} receipt`, async t => {
  const root = fixture(t); const r = receipt(root);
  if (mismatch === 'hash') r.sha256 = '0'.repeat(64);
  if (mismatch === 'lock') writeFileSync(join(root, 'remote-helper/Cargo.lock'), 'changed');
  if (mismatch === 'machine') { const b = readFileSync(r.executable); b.writeUInt16LE(0xaa64, 132); writeFileSync(r.executable, b); r.sha256 = computeSha256(b); }
  await assert.rejects(stage(root, { receipts: mismatch === 'duplicate' ? [r, r] : [r], requiredTargets: [target] }));
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
const CPU_TYPE = { 'aarch64-apple-darwin': 0x0100000c, 'x86_64-apple-darwin': 0x01000007 };
function machoReceipt(root, machoTarget, { magic = 0xfeedfacf, cpuType = CPU_TYPE[machoTarget] } = {}) {
  const path = join(root, `helper-${machoTarget}`);
  // Minimal 64-bit Mach-O header fixture; the ABI is asserted from bytes, never the triple string.
  const bytes = Buffer.alloc(256); bytes.writeUInt32LE(magic, 0); bytes.writeUInt32LE(cpuType, 4); writeFileSync(path, bytes);
  const sources = ['remote-helper/Cargo.toml', 'remote-helper/Cargo.lock'].map(path => ({ path, sha256: computeSha256(readFileSync(join(root, path))) }));
  return { target: machoTarget, profile: 'release', executable: path, sha256: computeSha256(bytes), sources };
}
for (const machoTarget of Object.keys(CPU_TYPE)) test(`stager publishes ${machoTarget} from an explicit Mach-O receipt`, async t => {
  const root = fixture(t); const r = machoReceipt(root, machoTarget);
  const manifest = await stage(root, { receipts: [r], requiredTargets: [machoTarget] });
  assert.equal(manifest.artifacts.length, 1);
  assert.equal(manifest.artifacts[0].target, machoTarget);
  assert.equal(manifest.artifacts[0].filename, 'ferryx-remote-helper');
  assert.equal(manifest.artifacts[0].sha256, r.sha256);
  assert.deepEqual(readFileSync(join(root, 'out', machoTarget, 'ferryx-remote-helper')), readFileSync(r.executable));
});
for (const bad of ['magic', 'cputype', 'elf']) test(`stager rejects darwin receipt with wrong ${bad}`, async t => {
  const root = fixture(t);
  const r = bad === 'cputype'
    ? machoReceipt(root, 'aarch64-apple-darwin', { cpuType: CPU_TYPE['x86_64-apple-darwin'] })
    : machoReceipt(root, 'aarch64-apple-darwin', { magic: bad === 'elf' ? 0x464c457f : 0xfeedface });
  await assert.rejects(stage(root, { receipts: [r], requiredTargets: ['aarch64-apple-darwin'] }));
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
test('stager still rejects a Mach-O artifact published under a linux target', async t => {
  const root = fixture(t); const r = machoReceipt(root, 'aarch64-apple-darwin');
  await assert.rejects(stage(root, { receipts: [{ ...r, target: 'aarch64-unknown-linux-gnu' }], requiredTargets: ['aarch64-unknown-linux-gnu'] }));
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
