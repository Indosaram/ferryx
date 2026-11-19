import test, { afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync,
} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { MANIFEST_NAME, sealArtifactSet, stageArtifactSet } from './lib/artifact-set.mjs';
import { generationKey } from './lib/wire.mjs';

const RUN = '20261019t064512z';
const OTHER_RUN = '20261019t064513z';
const OID = (c) => c.repeat(40);
const sha = (text) => createHash('sha256').update(text).digest('hex');
const noSymlinks = process.platform === 'win32';

const roots = new Set();
const tmp = () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'qa-artifact-set-'));
  roots.add(dir);
  return dir;
};
afterEach(() => {
  for (const root of roots) rmSync(root, { recursive: true, force: true });
  roots.clear();
});
const writeTree = (root, files) => {
  for (const [rel, content] of Object.entries(files)) {
    const abs = path.join(root, rel);
    mkdirSync(path.dirname(abs), { recursive: true });
    writeFileSync(abs, content);
  }
};

const identity = (overrides = {}) => ({
  platform: 'linux', arch: 'arm64', head: OID('a'), tree: OID('b'), runId: RUN, version: '0.1.0', v1Head: null, baselineHead: null,
  ...overrides,
});

const plist = (bundleId, executable) => `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>${executable}</string>
  <key>CFBundleIdentifier</key>
  <string>${bundleId}</string>
</dict>
</plist>
`;

const productName = (runId = RUN) => `Ferryx QA ${runId}`;

function seedLinux(directory, names = ['A', 'B', 'C', 'bump'], aliases = {}) {
  for (const name of names) {
    const salt = aliases[name] ?? name;
    writeTree(directory, {
      [`artifacts/${name}/ferryx`]: `gui-${salt}`,
      [`artifacts/${name}/ferryx-cli`]: `cli-${salt}`,
      // Incidental build metadata the executed package must not carry.
      [`artifacts/${name}/outer-entitlements.plist`]: `entitlements-${name}`,
    });
  }
}

function seedWindows(directory, names = ['A', 'B', 'C', 'bump']) {
  for (const name of names) {
    writeTree(directory, {
      [`artifacts/${name}/ferryx.exe`]: `gui-${name}`,
      [`artifacts/${name}/ferryx-cli.exe`]: `cli-${name}`,
      [`artifacts/${name}/avcodec-63.dll`]: `codec-${name}`,
      [`artifacts/${name}/avutil-59.dll`]: `util-${name}`,
      ...Object.fromEntries(['avformat', 'avfilter', 'avdevice', 'swresample', 'swscale'].map((lib) => [`artifacts/${name}/${lib}-42.dll`, `${lib}-${name}`])),
      [`artifacts/${name}/ferryx.pdb`]: `symbols-${name}`,
    });
  }
}

function seedMac(directory, names = ['A', 'B', 'C', 'bump'], { runId = RUN, bundleRunId = runId } = {}) {
  for (const name of names) {
    const app = `artifacts/${name}/${productName(runId)}.app`;
    writeTree(directory, {
      [`${app}/Contents/Info.plist`]: plist(`com.ferryx.app.qa-${bundleRunId}`, 'ferryx'),
      [`${app}/Contents/MacOS/ferryx`]: `gui-${name}`,
      [`${app}/Contents/Resources/ffmpeg/LICENSE`]: 'ffmpeg-license',
      [`${app}/Contents/Helpers/Ferryx Daemon.app/Contents/Info.plist`]: plist('com.ferryx.app.daemon', 'ferryx'),
      [`${app}/Contents/Helpers/Ferryx Daemon.app/Contents/MacOS/ferryx`]: `daemon-${name}`,
      [`artifacts/${name}/ferryx-cli`]: `cli-${name}`,
      [`artifacts/${name}/outer-entitlements.plist`]: `entitlements-${name}`,
    });
  }
}

/** Signature seam stand-in: records targets, reads real plists, optionally refuses one target. */
function fakeMac({ refuse } = {}) {
  const verified = [];
  return {
    verified,
    verify(target) {
      verified.push(target);
      if (refuse && target.includes(refuse)) return { status: 1, output: 'code object is not signed at all' };
      return { status: 0, output: '' };
    },
    plistValue(file, key) {
      const m = new RegExp(`<key>${key}</key>\\s*<string>([^<]*)</string>`).exec(readFileSync(file, 'utf8'));
      return m ? { status: 0, value: m[1] } : { status: 1, value: '' };
    },
  };
}

/** Rewrites a copy of `base` (the sealed text) through `mutate`, re-pins it, returns the new digest. */
function repin(directory, mutate, base, { canonical = true } = {}) {
  const manifest = JSON.parse(base);
  mutate(manifest);
  const text = canonical ? `${JSON.stringify(manifest)}\n` : `${JSON.stringify(manifest, null, 2)}\n`;
  writeFileSync(path.join(directory, MANIFEST_NAME), text);
  return { pin: sha(text), manifest };
}

/** A fresh, exclusively owned artifact root: <tmp>/run-artifacts/<fresh child>. */
function ownedRoot() {
  const root = tmp();
  const owned = path.join(root, 'run-artifacts');
  mkdirSync(owned);
  return owned;
}

test('linux: seal then stage reproduces GUI and CLI bytes and derives run.mjs descriptors', async () => {
  const build = tmp();
  const owned = ownedRoot();
  seedLinux(build);
  const id = identity();

  const sealed = await sealArtifactSet({ directory: build, identity: id });
  assert.equal(sealed.sha256, sha(`${JSON.stringify(sealed.manifest)}\n`));
  assert.equal(readFileSync(path.join(build, MANIFEST_NAME), 'utf8'), `${JSON.stringify(sealed.manifest)}\n`);
  assert.equal(sealed.manifest.schema, 'ferryx-qa-artifact-set/v1');
  assert.deepEqual(Object.keys(sealed.manifest.variants), ['A', 'B', 'C', 'bump']);
  for (const name of ['A', 'B', 'C', 'bump']) {
    assert.equal(sealed.manifest.variants[name].generationKey, generationKey('0.1.0', sha(`gui-${name}`)));
  }
  // Sealing twice over identical bytes is deterministic.
  assert.equal((await sealArtifactSet({ directory: build, identity: id })).sha256, sealed.sha256);

  const destination = path.join(owned, 'A-set');
  const staged = await stageArtifactSet({ directory: build, sha256: sealed.sha256, identity: id, destination });
  assert.equal(readFileSync(path.join(destination, MANIFEST_NAME), 'utf8'), `${JSON.stringify(sealed.manifest)}\n`);
  assert.deepEqual(readdirSync(destination).sort(), ['artifact-manifest.json', 'artifacts']);
  for (const name of ['A', 'B', 'C', 'bump']) {
    const art = staged.artifacts[name];
    assert.equal(art.name, name);
    assert.equal(art.appPath, null);
    assert.equal(art.guiExecutable, path.join(destination, 'artifacts', name, 'ferryx'));
    assert.equal(art.cliExecutable, path.join(destination, 'artifacts', name, 'ferryx-cli'));
    assert.equal(art.daemonSource, art.guiExecutable);
    assert.equal(readFileSync(art.guiExecutable, 'utf8'), `gui-${name}`);
    // The CLI is part of the executed package and must be staged verbatim.
    assert.equal(readFileSync(art.cliExecutable, 'utf8'), `cli-${name}`);
    assert.equal(art.sha256, sha(`gui-${name}`));
    assert.equal(staged.keys[name], generationKey('0.1.0', art.sha256));
    // Incidental build metadata stays out of the executed package.
    assert.equal(existsSync(path.join(destination, 'artifacts', name, 'outer-entitlements.plist')), false);
  }
  assert.equal(staged.manifest.identity.head, id.head);
});

test('windows: DLLs and CLI are sealed and staged beside the no-bundle GUI', async () => {
  const build = tmp();
  const owned = ownedRoot();
  seedWindows(build);
  const id = identity({ platform: 'windows' });
  const sealed = await sealArtifactSet({ directory: build, identity: id });
  assert.deepEqual(sealed.manifest.variants.A.entries.map((e) => e.path), ['avcodec-63.dll', 'avdevice-42.dll', 'avfilter-42.dll', 'avformat-42.dll', 'avutil-59.dll', 'ferryx-cli.exe', 'ferryx.exe', 'swresample-42.dll', 'swscale-42.dll']);

  const destination = path.join(owned, 'win-set');
  const staged = await stageArtifactSet({ directory: build, sha256: sealed.sha256, identity: id, destination });
  const a = staged.artifacts.A;
  assert.equal(a.appPath, null);
  assert.equal(a.guiExecutable, path.join(destination, 'artifacts', 'A', 'ferryx.exe'));
  assert.equal(readFileSync(a.guiExecutable, 'utf8'), 'gui-A');
  assert.equal(readFileSync(path.join(destination, 'artifacts', 'A', 'avcodec-63.dll'), 'utf8'), 'codec-A');
  assert.equal(readFileSync(a.cliExecutable, 'utf8'), 'cli-A');
  assert.equal(existsSync(path.join(destination, 'artifacts', 'A', 'ferryx.pdb')), false);
});

test('mac: app, nested daemon, CLI and an internal bundle symlink roundtrip; deep verify runs on both sides', { skip: noSymlinks }, async () => {
  const build = tmp();
  const owned = ownedRoot();
  seedMac(build);
  const app = `artifacts/A/${productName()}.app`;
  writeTree(build, {
    [`${app}/Contents/Frameworks/libav.61.dylib`]: 'ffmpeg-dylib-A',
    [`${app}/Contents/Frameworks/libav.61.framework/Versions/A/libav.61`]: 'framework-binary-A',
  });
  symlinkSync('Versions/A/libav.61', path.join(build, app, 'Contents/Frameworks/libav.61.framework/libav.61'));
  const id = identity({ platform: 'mac' });

  const sealTools = fakeMac();
  const sealed = await sealArtifactSet({ directory: build, identity: id, mac: sealTools });
  // Both the app and its nested daemon bundle are checked for every non-baseline variant.
  assert.deepEqual(sealTools.verified.map((t) => path.basename(t)).slice(0, 4), [
    `${productName()}.app`, 'Ferryx Daemon.app', `${productName()}.app`, 'Ferryx Daemon.app',
  ]);
  assert.equal(sealTools.verified.length, 8);
  const variant = sealed.manifest.variants.A;
  assert.equal(variant.bundleIdentifier, `com.ferryx.app.qa-${RUN}`);
  assert.equal(variant.artifacts.appPath, `${productName()}.app`);
  assert.equal(variant.artifacts.guiExecutable, `${productName()}.app/Contents/MacOS/ferryx`);
  assert.equal(variant.artifacts.daemonSource, `${productName()}.app/Contents/Helpers/Ferryx Daemon.app/Contents/MacOS/ferryx`);
  assert.equal(variant.daemon.sha256, sha('daemon-A'));
  assert.equal(variant.generationKey, generationKey('0.1.0', sha('daemon-A')));
  assert.deepEqual(variant.entries.filter((e) => e.type === 'symlink').map((e) => e.path), [
    `${productName()}.app/Contents/Frameworks/libav.61.framework/libav.61`,
  ]);

  const stageTools = fakeMac();
  const destination = path.join(owned, 'mac-set');
  const staged = await stageArtifactSet({ directory: build, sha256: sealed.sha256, identity: id, destination, mac: stageTools });
  assert.ok(stageTools.verified.some((t) => t.includes('Ferryx Daemon.app')));
  const a = staged.artifacts.A;
  assert.equal(a.appPath, path.join(destination, 'artifacts', 'A', `${productName()}.app`));
  assert.equal(readFileSync(a.guiExecutable, 'utf8'), 'gui-A');
  assert.equal(readFileSync(a.cliExecutable, 'utf8'), 'cli-A');
  assert.equal(readFileSync(a.daemonSource, 'utf8'), 'daemon-A');
  assert.equal(
    readFileSync(path.join(a.appPath, 'Contents/Frameworks/libav.61.framework/Versions/A/libav.61'), 'utf8'),
    'framework-binary-A',
  );
  assert.equal(existsSync(path.join(destination, 'artifacts', 'A', 'outer-entitlements.plist')), false);
});

test('mac: a rejected signature is refused at seal and again at stage', async () => {
  const build = tmp();
  const owned = ownedRoot();
  seedMac(build);
  const id = identity({ platform: 'mac' });
  await assert.rejects(
    sealArtifactSet({ directory: build, identity: id, mac: fakeMac({ refuse: 'Ferryx Daemon.app' }) }),
    /ARTIFACT_SET_SIGNATURE: codesign --verify --deep --strict refused .*Ferryx Daemon\.app/,
  );
  assert.equal(existsSync(path.join(build, MANIFEST_NAME)), false, 'a refused seal writes no manifest');

  const sealed = await sealArtifactSet({ directory: build, identity: id, mac: fakeMac() });
  const destination = path.join(owned, 'mac-rejected');
  await assert.rejects(
    stageArtifactSet({ directory: build, sha256: sealed.sha256, identity: id, destination, mac: fakeMac({ refuse: 'Ferryx Daemon.app' }) }),
    /ARTIFACT_SET_SIGNATURE/,
  );
  assert.equal(existsSync(destination), false, 'a refused stage leaves no destination behind');
});

test('mac: a foreign CFBundleIdentifier is refused even when the signature verifies', async () => {
  const build = tmp();
  seedMac(build, ['A', 'B', 'C', 'bump'], { bundleRunId: OTHER_RUN });
  await assert.rejects(
    sealArtifactSet({ directory: build, identity: identity({ platform: 'mac' }), mac: fakeMac() }),
    /ARTIFACT_SET_SIGNATURE: artifacts\/A\/.*CFBundleIdentifier is "com\.ferryx\.app\.qa-20261019t064513z"/,
  );
});

for (const [label, seed, platform, mutate] of [
  ['linux CLI', seedLinux, 'linux', (dir) => writeFileSync(path.join(dir, 'artifacts/A/ferryx-cli'), 'tampered-cli')],
  ['linux GUI', seedLinux, 'linux', (dir) => writeFileSync(path.join(dir, 'artifacts/B/ferryx'), 'tampered-gui')],
  ['windows shared library', seedWindows, 'windows', (dir) => writeFileSync(path.join(dir, 'artifacts/C/avcodec-63.dll'), 'tampered-dll')],
  ['mac bundled resource', seedMac, 'mac', (dir) => writeFileSync(path.join(dir, `artifacts/A/${productName()}.app/Contents/Resources/ffmpeg/LICENSE`), 'tampered-resource')],
  ['mac nested daemon', seedMac, 'mac', (dir) => writeFileSync(path.join(dir, `artifacts/bump/${productName()}.app/Contents/Helpers/Ferryx Daemon.app/Contents/MacOS/ferryx`), 'tampered-daemon')],
]) {
  test(`${label}: payload tampering after seal is refused by the re-read hash, not merely by presence`, async () => {
    const build = tmp();
    const owned = ownedRoot();
    seed(build);
    const id = identity({ platform });
    const mac = platform === 'mac' ? fakeMac() : undefined;
    const sealed = await sealArtifactSet({ directory: build, identity: id, mac });
    mutate(build);
    const destination = path.join(owned, 'tampered');
    await assert.rejects(
      stageArtifactSet({ directory: build, sha256: sealed.sha256, identity: id, destination, mac }),
      { code: 'ARTIFACT_SET_PAYLOAD' },
    );
    assert.equal(existsSync(destination), false);
  });
}

test('variant sets are refused: missing, unexpected, duplicated generation keys, nonregular fixed entries', async () => {
  const id = identity();
  const missing = tmp();
  seedLinux(missing, ['A', 'B', 'C']);
  await assert.rejects(sealArtifactSet({ directory: missing, identity: id }), /missing variant bump/);

  const unexpected = tmp();
  seedLinux(unexpected);
  seedLinux(unexpected, ['D']);
  await assert.rejects(sealArtifactSet({ directory: unexpected, identity: id }), /unexpected variant D/);

  const duplicate = tmp();
  seedLinux(duplicate);
  writeFileSync(path.join(duplicate, 'artifacts/B/ferryx'), 'gui-A');
  await assert.rejects(sealArtifactSet({ directory: duplicate, identity: id }), /duplicated generation keys: A and B/);

  const directoryEntry = tmp();
  seedLinux(directoryEntry);
  rmSync(path.join(directoryEntry, 'artifacts/C/ferryx-cli'));
  mkdirSync(path.join(directoryEntry, 'artifacts/C/ferryx-cli'));
  await assert.rejects(sealArtifactSet({ directory: directoryEntry, identity: id }), /artifacts\/C\/ferryx-cli must be a real regular file/);
});

test('a symlink where a fixed entry belongs is refused at seal', { skip: noSymlinks }, async () => {
  const build = tmp();
  seedLinux(build);
  rmSync(path.join(build, 'artifacts/A/ferryx'));
  symlinkSync('ferryx-cli', path.join(build, 'artifacts/A/ferryx'));
  await assert.rejects(sealArtifactSet({ directory: build, identity: identity() }), /artifacts\/A\/ferryx must be a real regular file/);
});

test('baseline holds only the daemon executable and carries no generation key', async () => {
  const build = tmp();
  const owned = ownedRoot();
  seedLinux(build);
  writeTree(build, { 'artifacts/baseline/ferryx': 'baseline-daemon' });
  const id = identity({ baselineHead: OID('c') });

  const sealed = await sealArtifactSet({ directory: build, identity: id });
  assert.deepEqual(Object.keys(sealed.manifest.variants), ['A', 'B', 'C', 'bump', 'baseline']);
  assert.equal(sealed.manifest.variants.baseline.generationKey, null);
  assert.equal(sealed.manifest.variants.baseline.artifacts, null);
  assert.deepEqual(sealed.manifest.variants.baseline.entries.map((e) => e.path), ['ferryx']);

  const staged = await stageArtifactSet({ directory: build, sha256: sealed.sha256, identity: id, destination: path.join(owned, 'with-baseline') });
  assert.equal(staged.keys.baseline, undefined, 'the baseline has no generation key');
  assert.deepEqual(staged.artifacts.baseline, {
    name: 'baseline', daemonExecutable: path.join(owned, 'with-baseline', 'artifacts', 'baseline', 'ferryx'),
  });

  const extra = tmp();
  seedLinux(extra);
  seedLinux(extra, ['baseline']);
  await assert.rejects(
    sealArtifactSet({ directory: extra, identity: identity({ baselineHead: OID('c') }) }),
    /artifacts\/baseline must hold only ferryx/,
  );
});

test('optional variants and every identity field must match the pinned set', async () => {
  const build = tmp();
  seedLinux(build, ['A', 'B', 'C', 'bump', 'v1bump']);
  // A v1bump directory with no v1Head pin is an unexpected variant.
  await assert.rejects(sealArtifactSet({ directory: build, identity: identity() }), /unexpected variant v1bump/);

  const pinned = tmp();
  seedLinux(pinned, ['A', 'B', 'C', 'bump', 'v1bump']);
  writeFileSync(path.join(pinned, 'artifacts/v1bump/ferryx'), 'gui-v1bump');
  const sealed = await sealArtifactSet({ directory: pinned, identity: identity({ v1Head: OID('d') }) });
  assert.deepEqual(Object.keys(sealed.manifest.variants), ['A', 'B', 'C', 'bump', 'v1bump']);

  const stage = (overrides) => stageArtifactSet({
    directory: pinned, sha256: sealed.sha256, identity: identity({ v1Head: OID('d'), ...overrides }), destination: path.join(ownedRoot(), 'id'),
  });
  // A consumer whose identity dropped the v1 pin cannot consume the set...
  await assert.rejects(stage({ v1Head: null }), /ARTIFACT_SET_IDENTITY: manifest v1Head "[0-9a-f]{40}" is not this run's null/);
  // ...and every other identity difference is refused before any byte is copied.
  for (const [field, value] of [['head', OID('e')], ['tree', OID('f')], ['arch', 'x64'], ['platform', 'mac'], ['version', '0.2.0'], ['runId', OTHER_RUN], ['baselineHead', OID('0')]]) {
    await assert.rejects(stage({ [field]: value }), /ARTIFACT_SET_IDENTITY/, field);
  }
  // Without the optional variant the pin cannot match either.
  const plain = tmp();
  seedLinux(plain);
  const plainSealed = await sealArtifactSet({ directory: plain, identity: identity() });
  await assert.rejects(
    stageArtifactSet({ directory: plain, sha256: plainSealed.sha256, identity: identity({ v1Head: OID('d') }), destination: path.join(ownedRoot(), 'no-v1bump') }),
    /ARTIFACT_SET_IDENTITY: manifest v1Head null is not this run's/,
  );
});

test('malformed, non-canonical and unsupported manifests are refused', async () => {
  const build = tmp();
  seedLinux(build);
  const id = identity();
  const sealed = await sealArtifactSet({ directory: build, identity: id });
  const base = readFileSync(path.join(build, MANIFEST_NAME), 'utf8');
  const dest = () => path.join(ownedRoot(), 'malformed');
  const stage = (pin) => stageArtifactSet({ directory: build, sha256: pin, identity: id, destination: dest() });

  writeFileSync(path.join(build, MANIFEST_NAME), '{ not json');
  await assert.rejects(stage(sha('{ not json')), /ARTIFACT_SET_MANIFEST: .*is not JSON/);

  await assert.rejects(stage(repin(build, () => {}, base, { canonical: false }).pin), /is not in the canonical sealed format/);
  await assert.rejects(stage(repin(build, (m) => { m.schema = 'ferryx-qa-artifact-set/v2'; }, base).pin), /unsupported schema "ferryx-qa-artifact-set\/v2"/);
  await assert.rejects(stage(repin(build, (m) => { m.variants.A.extra = true; }, base).pin), /variants\.A has an unexpected field extra/);
  await assert.rejects(stage(repin(build, (m) => { m.variants.A.entries[0].type = 'device'; }, base).pin), /has unsupported type "device"/);
  await assert.rejects(stage(repin(build, (m) => { m.variants.B.generationKey = '0.1.0-deadbeef'; }, base).pin), /variants\.B\.generationKey .* is not "0\.1\.0-/);
  await assert.rejects(stage(repin(build, (m) => { delete m.variants.bump; }, base).pin), /variants is missing bump/);
  await assert.rejects(stage(repin(build, (m) => { m.variants.A.daemon.sha256 = sha('other'); }, base).pin), /daemon sha256 is not the sealed file digest/);
  assert.equal(sealed.sha256.length, 64);
  assert.equal(existsSync(dest()), false);
});

test('manifest-supplied paths never escape the destination', async () => {
  const build = tmp();
  seedLinux(build);
  const id = identity();
  await sealArtifactSet({ directory: build, identity: id });
  const base = readFileSync(path.join(build, MANIFEST_NAME), 'utf8');
  const dest = () => path.join(ownedRoot(), 'escape');

  for (const [label, badPath] of [['traversal', '../ferryx-cli'], ['absolute', '/etc/passwd'], ['backslash', 'artifacts\\A\\ferryx'], ['dot-segment', 'x/../../ferryx']]) {
    const mutated = repin(build, (m) => { m.variants.A.entries.find((e) => e.path === 'ferryx-cli').path = badPath; }, base);
    await assert.rejects(
      stageArtifactSet({ directory: build, sha256: mutated.pin, identity: id, destination: dest() }),
      /ARTIFACT_SET_PATH/,
      label,
    );
    assert.equal(existsSync(dest()), false, `${label}: nothing is created`);
  }

  // A file outside the fixed expected entries (here an incidental build file) is refused too.
  const outside = repin(build, (m) => {
    m.variants.A.entries.push({ path: 'outer-entitlements.plist', type: 'file', size: 13, mode: 0o644, sha256: sha('entitlements-A') });
    m.variants.A.entries.sort((a, b) => (a.path < b.path ? -1 : 1));
  }, base);
  await assert.rejects(
    stageArtifactSet({ directory: build, sha256: outside.pin, identity: id, destination: dest() }),
    /outside the expected entries/,
  );
  assert.equal(existsSync(dest()), false);
});

test('mac manifest symlinks must stay inside the app bundle', { skip: noSymlinks }, async () => {
  const build = tmp();
  seedMac(build);
  const app = `artifacts/A/${productName()}.app`;
  writeTree(build, { [`${app}/Contents/Frameworks/libav.61.dylib`]: 'ffmpeg-dylib-A' });
  const id = identity({ platform: 'mac' });
  await sealArtifactSet({ directory: build, identity: id, mac: fakeMac() });
  const base = readFileSync(path.join(build, MANIFEST_NAME), 'utf8');
  const dest = () => path.join(ownedRoot(), 'link-escape');

  for (const target of ['../../../../etc/passwd', '/etc/passwd']) {
    const mutated = repin(build, (m) => {
      m.variants.A.entries.push({ path: `${productName()}.app/Contents/Frameworks/evil`, type: 'symlink', link: target });
      m.variants.A.entries.sort((a, b) => (a.path < b.path ? -1 : 1));
    }, base);
    await assert.rejects(
      stageArtifactSet({ directory: build, sha256: mutated.pin, identity: id, destination: dest(), mac: fakeMac() }),
      /ARTIFACT_SET_PATH: .*symlink target/,
      target,
    );
    assert.equal(existsSync(dest()), false);
  }
});

test('an escaping, absolute or dangling build-time symlink is refused at seal', { skip: noSymlinks }, async () => {
  for (const target of ['../../../outside', '/etc/passwd', 'missing-target']) {
    const build = tmp();
    seedMac(build);
    const link = path.join(build, `artifacts/A/${productName()}.app/Contents/Frameworks/libav.61.dylib`);
    mkdirSync(path.dirname(link), { recursive: true });
    symlinkSync(target, link);
    await assert.rejects(
      sealArtifactSet({ directory: build, identity: identity({ platform: 'mac' }), mac: fakeMac() }),
      /ARTIFACT_SET_PATH: (absolute symlink|symlink .* escapes|dangling symlink)/,
      target,
    );
  }
});

test('a mismatched pinned manifest sha256 is refused before any copy', async () => {
  const build = tmp();
  seedLinux(build);
  const id = identity();
  const sealed = await sealArtifactSet({ directory: build, identity: id });
  const destination = path.join(ownedRoot(), 'pinned');
  const wrong = `${sealed.sha256[0] === '0' ? '1' : '0'}${sealed.sha256.slice(1)}`;
  await assert.rejects(
    stageArtifactSet({ directory: build, sha256: wrong, identity: id, destination }),
    /ARTIFACT_SET_MANIFEST: manifest sha256 .* does not match the pinned/,
  );
  await assert.rejects(
    stageArtifactSet({ directory: build, sha256: 'not-a-digest', identity: id, destination }),
    /pinned manifest sha256 must be 64 lowercase hex/,
  );
  assert.equal(existsSync(destination), false);
});

test('unsafe or occupied destinations are refused and left alone', async () => {
  const build = tmp();
  seedLinux(build);
  const id = identity();
  const sealed = await sealArtifactSet({ directory: build, identity: id });
  const stage = (destination) => stageArtifactSet({ directory: build, sha256: sealed.sha256, identity: id, destination });

  const occupied = path.join(ownedRoot(), 'occupied');
  mkdirSync(occupied);
  writeFileSync(path.join(occupied, 'keep.txt'), 'mine');
  await assert.rejects(stage(occupied), /ARTIFACT_SET_DESTINATION: .*already exists/);
  assert.equal(readFileSync(path.join(occupied, 'keep.txt'), 'utf8'), 'mine');

  await assert.rejects(stage(path.join(build, 'inside-build')), /ARTIFACT_SET_DESTINATION: .*overlaps the sealed build directory/);
  await assert.rejects(stage(path.join(path.parse(os.tmpdir()).root, 'qa-artifact-dest')), /too close to the filesystem root/);
  await assert.rejects(stage(path.join(os.homedir(), 'Downloads', 'qa-artifact-dest')), /is inside/);
  await assert.rejects(stage('relative/dest'), /must be an absolute path/);
  await assert.rejects(stage(path.join(tmp(), 'missing-parent', 'dest')), /parent of .* does not exist/);
});
