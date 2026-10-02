import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import vm from 'node:vm';
import {
  stageHelpers,
  computeSha256,
  computeHelperSourceClosure,
  computeHelperSourceFingerprint,
  validateBundledHelpers,
  REQUIRED_HELPER_CAPABILITIES,
} from './build-remote-helpers.mjs';

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
// Single source for the fixture's packaged version: the stager reads it back from
// remote-helper/Cargo.toml, so every receipt must agree with this value.
const FIXTURE_HELPER_VERSION = '2026.908.1';
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'p25-stage-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));

  const closure = computeHelperSourceClosure('.');
  for (const rel of closure) {
    const full = join(root, rel);
    mkdirSync(join(full, '..'), { recursive: true });
    if (rel === 'remote-helper/Cargo.toml') {
      writeFileSync(full, `[package]\nname = "ferryx-remote-helper"\nversion = "${FIXTURE_HELPER_VERSION}"\n`);
    } else if (rel === 'remote-helper/Cargo.lock') {
      writeFileSync(full, 'owned lock');
    } else {
      writeFileSync(full, `// mock source: ${rel}\n`);
    }
  }

  return root;
}
for (const mode of ['empty', 'gnu', 'debug', 'stale']) test(`stager rejects ${mode} without publication`, async t => {
  const root = fixture(t);
  const rel = mode === 'gnu' ? 'remote-helper/target/x86_64-pc-windows-gnu/release' : mode === 'debug' ? `remote-helper/target/${target}/debug` : `src-tauri/resources/helpers/${target}`;
  if (mode !== 'empty') { mkdirSync(join(root, rel), { recursive: true }); writeFileSync(join(root, rel, 'ferryx-remote-helper.exe'), 'stale'); }
  await assert.rejects(stage(root, { requiredTargets: [target], receipts: [] }));
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
function receipt(root, { capabilities = [...REQUIRED_HELPER_CAPABILITIES], helperVersion = FIXTURE_HELPER_VERSION } = {}) {
  const path = join(root, 'helper.exe');
  // Minimal PE machine fixture. Capability evidence travels as measured data, never
  // as marker bytes in the image: the optimizer may split or merge constants.
  const bytes = Buffer.alloc(256);
  bytes.write('MZ');
  bytes.writeUInt32LE(128, 60);
  bytes.write('PE\0\0', 128);
  bytes.writeUInt16LE(0x8664, 132);
  writeFileSync(path, bytes);
  const { fingerprint: sourceFingerprint, sources } = computeHelperSourceFingerprint(root);
  return { target, profile: 'release', executable: path, sha256: computeSha256(bytes), helperVersion, capabilities, sourceFingerprint, sources };
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
  assert.equal(manifest.sourceFingerprint, r.sourceFingerprint);
  assert.deepEqual(manifest.requiredCapabilities, REQUIRED_HELPER_CAPABILITIES);
  assert.deepEqual(readFileSync(join(root, 'out', target, 'ferryx-remote-helper.exe')), readFileSync(r.executable));
});
test('stager records the measured capability surface in the manifest', async t => {
  const root = fixture(t);
  const r = receipt(root, { capabilities: [...REQUIRED_HELPER_CAPABILITIES, 'dagStreamingV1'] });
  const manifest = await stage(root, { receipts: [r], requiredTargets: [target] });
  assert.deepEqual(manifest.artifacts[0].capabilities, ['sshHelperV1', 'dagSubscribeV1', 'agentStateV1', 'ptyRecoveryV1', 'dagStreamingV1']);
});
test('stager rejects a receipt missing reboot recovery capability evidence', async t => {
  const root = fixture(t);
  const r = receipt(root, { capabilities: REQUIRED_HELPER_CAPABILITIES.filter(capability => capability !== 'ptyRecoveryV1') });
  await assert.rejects(stage(root, { receipts: [r], requiredTargets: [target] }));
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
for (const mismatch of ['hash', 'lock', 'machine', 'duplicate', 'missing_closure', 'stale_source']) test(`stager rejects ${mismatch} receipt`, async t => {
  const root = fixture(t); const r = receipt(root);
  if (mismatch === 'hash') r.sha256 = '0'.repeat(64);
  if (mismatch === 'lock') writeFileSync(join(root, 'remote-helper/Cargo.lock'), 'changed');
  if (mismatch === 'machine') { const b = readFileSync(r.executable); b.writeUInt16LE(0xaa64, 132); writeFileSync(r.executable, b); r.sha256 = computeSha256(b); }
  if (mismatch === 'missing_closure') { r.sources = r.sources.filter(s => s.path.endsWith('.toml') || s.path.endsWith('.lock')); }
  if (mismatch === 'stale_source') { writeFileSync(join(root, 'src-tauri/src/ferryx_scope/ssh/helper.rs'), '// modified source\n'); }
  await assert.rejects(stage(root, { receipts: mismatch === 'duplicate' ? [r, r] : [r], requiredTargets: [target] }));
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
const CPU_TYPE = { 'aarch64-apple-darwin': 0x0100000c, 'x86_64-apple-darwin': 0x01000007 };
function machoReceipt(root, machoTarget, { magic = 0xfeedfacf, cpuType = CPU_TYPE[machoTarget], capabilities = [...REQUIRED_HELPER_CAPABILITIES], helperVersion = FIXTURE_HELPER_VERSION } = {}) {
  const path = join(root, `helper-${machoTarget}`);
  // Minimal 64-bit Mach-O header fixture; the ABI is asserted from bytes, never the triple string.
  const bytes = Buffer.alloc(256);
  bytes.writeUInt32LE(magic, 0);
  bytes.writeUInt32LE(cpuType, 4);
  writeFileSync(path, bytes);
  const { fingerprint: sourceFingerprint, sources } = computeHelperSourceFingerprint(root);
  return { target: machoTarget, profile: 'release', executable: path, sha256: computeSha256(bytes), helperVersion, capabilities, sourceFingerprint, sources };
}
for (const machoTarget of Object.keys(CPU_TYPE)) test(`stager publishes ${machoTarget} from an explicit Mach-O receipt`, async t => {
  const root = fixture(t); const r = machoReceipt(root, machoTarget);
  const manifest = await stage(root, { receipts: [r], requiredTargets: [machoTarget] });
  assert.equal(manifest.artifacts.length, 1);
  assert.equal(manifest.artifacts[0].target, machoTarget);
  assert.equal(manifest.artifacts[0].filename, 'ferryx-remote-helper');
  assert.equal(manifest.artifacts[0].sha256, r.sha256);
  assert.equal(manifest.sourceFingerprint, r.sourceFingerprint);
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
test('stager rejects a receipt missing dagSubscribeV1 evidence even with matching version', async t => {
  const root = fixture(t);
  const r = receipt(root, { capabilities: ['sshHelperV1', 'agentStateV1'] });
  await assert.rejects(
    stage(root, { receipts: [r], requiredTargets: [target] }),
    { code: 'ERR_ASSERTION', actual: false, expected: true },
  );
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
test('stager rejects a receipt missing agentStateV1 evidence', async t => {
  const root = fixture(t);
  const r = receipt(root, { capabilities: ['sshHelperV1', 'dagSubscribeV1'] });
  await assert.rejects(
    stage(root, { receipts: [r], requiredTargets: [target] }),
    { code: 'ERR_ASSERTION', actual: false, expected: true },
  );
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
test('stager rejects a receipt without measured capability evidence', async t => {
  const root = fixture(t);
  const r = receipt(root);
  delete r.capabilities;
  await assert.rejects(
    stage(root, { receipts: [r], requiredTargets: [target] }),
    { code: 'ERR_ASSERTION', actual: false, expected: true },
  );
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
test('stager rejects a receipt without a measured helper version', async t => {
  const root = fixture(t);
  const r = receipt(root);
  delete r.helperVersion;
  await assert.rejects(
    stage(root, { receipts: [r], requiredTargets: [target] }),
    { code: 'ERR_ASSERTION', actual: undefined, expected: FIXTURE_HELPER_VERSION },
  );
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
test('stager rejects a receipt whose measured helper version disagrees with the packaged version', async t => {
  const root = fixture(t);
  const r = receipt(root, { helperVersion: '2026.930.1' });
  await assert.rejects(
    stage(root, { receipts: [r], requiredTargets: [target] }),
    { code: 'ERR_ASSERTION', actual: '2026.930.1', expected: FIXTURE_HELPER_VERSION },
  );
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
test('stager rejects artifact receipt whose sources match old subset instead of full closure', async t => {
  const root = fixture(t);
  const r = receipt(root);
  // Simulate legacy receipt that only recorded Cargo.toml and Cargo.lock
  r.sources = [
    { path: 'remote-helper/Cargo.toml', sha256: computeSha256(readFileSync(join(root, 'remote-helper/Cargo.toml'))) },
    { path: 'remote-helper/Cargo.lock', sha256: computeSha256(readFileSync(join(root, 'remote-helper/Cargo.lock'))) },
  ];
  await assert.rejects(
    stage(root, { receipts: [r], requiredTargets: [target] }),
    /source closure mismatch/,
  );
  assert.equal(existsSync(join(root, 'out/manifest.json')), false);
});
test('validateBundledHelpers permits absent manifest in development', () => {
  const root = mkdtempSync(join(tmpdir(), 'p25-dev-'));
  try {
    const res = validateBundledHelpers({ repoRoot: root, resourcesDir: join(root, 'resources') });
    assert.equal(res.ok, true);
    assert.equal(res.reason, 'no_manifest');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
test('validateBundledHelpers rejects manifest missing sourceFingerprint', (t) => {
  const root = fixture(t);
  const resourcesDir = join(root, 'src-tauri/resources/helpers');
  mkdirSync(resourcesDir, { recursive: true });
  writeFileSync(join(resourcesDir, 'manifest.json'), JSON.stringify({ schemaVersion: 1, helperVersion: '2026.917.1', protocolVersion: 1, artifacts: [] }));
  assert.throws(
    () => validateBundledHelpers({ repoRoot: root, resourcesDir }),
    /missing sourceFingerprint/,
  );
});
test('validateBundledHelpers rejects manifest with stale sourceFingerprint', (t) => {
  const root = fixture(t);
  const resourcesDir = join(root, 'src-tauri/resources/helpers');
  mkdirSync(resourcesDir, { recursive: true });
  writeFileSync(
    join(resourcesDir, 'manifest.json'),
    JSON.stringify({ schemaVersion: 1, helperVersion: '2026.917.1', protocolVersion: 1, sourceFingerprint: '0'.repeat(64), artifacts: [] }),
  );
  assert.throws(
    () => validateBundledHelpers({ repoRoot: root, resourcesDir }),
    /stale bundled helper assets detected/,
  );
});
test('validateBundledHelpers approves matching staged assets', async (t) => {
  const root = fixture(t);
  const r = receipt(root);
  const resourcesDir = join(root, 'src-tauri/resources/helpers');
  await stageHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target], receipts: [r] });
  const res = validateBundledHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target] });
  assert.equal(res.ok, true);
  assert.equal(res.sourceFingerprint, r.sourceFingerprint);
});
test('validateBundledHelpers rejects a bundled artifact missing a recorded capability', async t => {
  const root = fixture(t);
  const r = receipt(root);
  const resourcesDir = join(root, 'src-tauri/resources/helpers');
  await stageHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target], receipts: [r] });
  const manifestPath = join(resourcesDir, 'manifest.json');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  manifest.artifacts[0].capabilities = ['sshHelperV1', 'dagSubscribeV1'];
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  assert.throws(
    () => validateBundledHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target] }),
    { code: 'ERR_ASSERTION', actual: false, expected: true },
  );
});
test('validateBundledHelpers rejects a bundled artifact without recorded capability evidence', async t => {
  const root = fixture(t);
  const r = receipt(root);
  const resourcesDir = join(root, 'src-tauri/resources/helpers');
  await stageHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target], receipts: [r] });
  const manifestPath = join(resourcesDir, 'manifest.json');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  delete manifest.artifacts[0].capabilities;
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  assert.throws(
    () => validateBundledHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target] }),
    { code: 'ERR_ASSERTION', actual: false, expected: true },
  );
});
test('validateBundledHelpers ignores an emptied requiredCapabilities claim', async t => {
  const root = fixture(t);
  const r = receipt(root);
  const resourcesDir = join(root, 'src-tauri/resources/helpers');
  await stageHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target], receipts: [r] });
  const manifestPath = join(resourcesDir, 'manifest.json');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  manifest.requiredCapabilities = [];
  manifest.artifacts[0].capabilities = ['sshHelperV1'];
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  assert.throws(
    () => validateBundledHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target] }),
    { code: 'ERR_ASSERTION', actual: false, expected: true },
  );
});
test('validateBundledHelpers rejects an emptied requiredCapabilities claim with empty artifact evidence', async t => {
  const root = fixture(t);
  const r = receipt(root);
  const resourcesDir = join(root, 'src-tauri/resources/helpers');
  await stageHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target], receipts: [r] });
  const manifestPath = join(resourcesDir, 'manifest.json');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  manifest.requiredCapabilities = [];
  manifest.artifacts[0].capabilities = [];
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  assert.throws(
    () => validateBundledHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target] }),
    { code: 'ERR_ASSERTION', actual: false, expected: true },
  );
});
test('validateBundledHelpers rejects a valid bundle whose helperVersion disagrees with remote-helper/Cargo.toml', async (t) => {
  const root = fixture(t);
  const r = receipt(root);
  const resourcesDir = join(root, 'src-tauri/resources/helpers');
  await stageHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target], receipts: [r] });
  const manifestPath = join(resourcesDir, 'manifest.json');
  const staged = JSON.parse(readFileSync(manifestPath, 'utf8'));
  // The fixture manifest of record declares 2026.908.1; the bundle is otherwise valid.
  assert.equal(staged.helperVersion, '2026.908.1');
  assert.equal(staged.sourceFingerprint, r.sourceFingerprint);
  assert.equal(staged.artifacts[0].sha256, r.sha256);

  // Same bytes, same fingerprint, same hash: only the version field drifts.
  writeFileSync(manifestPath, JSON.stringify({ ...staged, helperVersion: '2026.930.1' }, null, 2) + '\n');
  assert.throws(
    () => validateBundledHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target] }),
    { code: 'ERR_ASSERTION', actual: '2026.930.1', expected: '2026.908.1' },
  );

  // Restoring the recorded version is the only change; the bundle then verifies again.
  writeFileSync(manifestPath, JSON.stringify(staged, null, 2) + '\n');
  assert.equal(validateBundledHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target] }).ok, true);
});
test('source closure and fingerprint ignore AppleDouble sidecars from foreign extractions', async (t) => {
  const root = fixture(t);
  const before = computeHelperSourceFingerprint(root);
  const sshDir = join(root, 'src-tauri/src/ferryx_scope/ssh');
  // bsdtar writes these when a snapshot is unpacked on a foreign host; the bytes
  // carry host metadata, so a foreign extraction must not move the fingerprint.
  writeFileSync(join(sshDir, '._helper.rs'), Buffer.from([0, 5, 22, 7, 0, 2, 0, 0, 109, 97, 99]));
  writeFileSync(join(sshDir, '._dag_stream.rs'), Buffer.from('linux-metadata'));
  mkdirSync(join(sshDir, '._helpers'), { recursive: true });
  writeFileSync(join(sshDir, '._helpers', 'inner.rs'), '// not source');

  const after = computeHelperSourceFingerprint(root);
  assert.equal(after.fingerprint, before.fingerprint);
  assert.deepEqual(after.sources, before.sources);
  assert.equal(after.sources.some(s => s.path.includes('/._')), false);

  // A receipt captured on either host describes the same source set and still stages.
  const r = receipt(root);
  assert.deepEqual(r.sources.map(s => s.path), before.sources.map(s => s.path));
  const manifest = await stage(root, { receipts: [r], requiredTargets: [target] });
  assert.equal(manifest.sourceFingerprint, before.fingerprint);
});
test('validateBundledHelpers rejects an incomplete shipped target matrix', async t => {
  const root = fixture(t);
  const r = receipt(root);
  const resourcesDir = join(root, 'src-tauri/resources/helpers');
  await stageHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target], receipts: [r] });
  let failure;
  try { validateBundledHelpers({ repoRoot: root, resourcesDir }); } catch (error) { failure = error; }
  assert.equal(failure?.code, 'ERR_ASSERTION');
  assert.deepEqual(failure.actual, [target]);
  assert.equal(failure.expected.length, 5);
});
test('validateBundledHelpers rejects a duplicated shipped target', async t => {
  const root = fixture(t);
  const r = receipt(root);
  const resourcesDir = join(root, 'src-tauri/resources/helpers');
  await stageHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target], receipts: [r] });
  const manifestPath = join(resourcesDir, 'manifest.json');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  manifest.artifacts.push({ ...manifest.artifacts[0] });
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  let failure;
  try {
    validateBundledHelpers({ repoRoot: root, resourcesDir, requiredTargets: [target, 'aarch64-unknown-linux-gnu'] });
  } catch (error) { failure = error; }
  assert.equal(failure?.code, 'ERR_ASSERTION');
  assert.deepEqual(failure.actual, [target, target]);
  assert.equal(failure.expected.length, 2);
});
