import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
function owned(t) {
  const root = mkdtempSync(join(tmpdir(), 'p18-owned-'));
  t.after(() => { rmSync(root, { recursive: true }); assert.equal(existsSync(root), false); });
  return root;
}
function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', timeout: 60000, ...options });
  assert.ifError(result.error);
  return result;
}

test('clean-layout edge contract compiles and executes without ephemeral evidence', t => {
  const root = owned(t);
  mkdirSync(join(root, 'src-tauri/tests'), { recursive: true });
  mkdirSync(join(root, 'scripts/fixtures'), { recursive: true });
  copyFileSync(join(repo, 'src-tauri/tests/windows_edge_probe_contract.rs'), join(root, 'src-tauri/tests/windows_edge_probe_contract.rs'));
  for (const name of ['run-edge-probes.mjs', 'probe-daemon-edges.mjs']) {
    const source = join(repo, 'scripts/fixtures', name);
    if (existsSync(source)) copyFileSync(source, join(root, 'scripts/fixtures', name));
  }
  const binary = join(root, 'edge-contract');
  const compile = run('rustc', ['--edition=2021', '--test', join(root, 'src-tauri/tests/windows_edge_probe_contract.rs'), '-o', binary], { env: { ...process.env, CARGO_MANIFEST_DIR: join(root, 'src-tauri') } });
  assert.equal(compile.status, 0, compile.stderr);
  const sentinel = join(root, 'foreign-sentinel');
  writeFileSync(sentinel, 'not owned by the driver');
  const execution = run(binary, ['--nocapture']);
  assert.equal(execution.status, 0, execution.stdout + execution.stderr);
  assert.match(execution.stdout, /1 passed/);
  assert.equal(readFileSync(sentinel, 'utf8'), 'not owned by the driver');
});

for (const [os, abi, manifests] of [['windows', 'msvc', true], ['windows', 'gnu', false], ['linux', 'gnu', false]]) {
  test(`build script emits MSVC manifest arguments only for ${os}/${abi}`, t => {
    const root = owned(t);
    // Execute the real build-script control flow; external Tauri/Ghostty builders
    // are replaced, not the environment checks or emitted Cargo directives.
    const source = readFileSync(join(repo, 'src-tauri/build.rs'), 'utf8').replace(
      '#[path = "native_terminal/build_ghostty.rs"]\nmod build_ghostty;',
      `mod build_ghostty { pub fn build_ghostty_vt() -> Result<(), String> { Ok(()) } }
mod tauri_build {
 pub struct Attributes; pub struct WindowsAttributes;
 impl Attributes { pub fn new() -> Self { Self } pub fn windows_attributes(self, _: WindowsAttributes) -> Self { self } }
 impl WindowsAttributes { pub fn new_without_app_manifest() -> Self { Self } }
 pub fn try_build(_: Attributes) -> Result<(), String> { println!("BUILDER=custom"); Ok(()) }
 pub fn build() { println!("BUILDER=default"); }
}`);
    writeFileSync(join(root, 'build.rs'), source);
    const binary = join(root, 'build-probe');
    const compile = run('rustc', ['--edition=2021', join(root, 'build.rs'), '-o', binary]);
    assert.equal(compile.status, 0, compile.stderr);
    const env = { ...process.env, OUT_DIR: root, CARGO_CFG_TARGET_OS: os, CARGO_CFG_TARGET_ENV: abi };
    delete env.CARGO_FEATURE_NATIVE_TERMINAL;
    const result = run(binary, [], { env });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.includes('/MANIFEST'), manifests, result.stdout);
    assert.match(result.stdout, manifests ? /BUILDER=custom/ : /BUILDER=default/);
  });
}

for (const mode of ['old', 'recent', 'invalid']) {
  test(`quiescence reports ${mode} metadata with GNU-compatible stat`, t => {
    const root = owned(t);
    mkdirSync(join(root, 'scripts'));
    mkdirSync(join(root, 'ui/src'), { recursive: true });
    mkdirSync(join(root, 'bin'));
    copyFileSync(join(repo, 'scripts/check-tree-quiescent.sh'), join(root, 'scripts/check-tree-quiescent.sh'));
    // Darwin lacks GNU stat: reject BSD flags and implement only the GNU
    // machine-consumed metadata format using the same filesystem metadata.
    writeFileSync(join(root, 'bin/stat'), `#!${process.execPath}\nconst fs=require('fs'); const args=process.argv.slice(2); if(args[0]!=='-c') process.exit(1); const p=args.at(-1); const s=fs.statSync(p); console.log(Math.floor(s.mtimeMs/1000)+'\\t'+s.mtime.toISOString()+'\\t'+p);\n`, { mode: 0o755 });
    const file = join(root, 'ui/src/space name.ts');
    writeFileSync(file, 'fixture');
    const date = mode === 'old' ? new Date('2000-01-01T00:00:00Z') : new Date();
    utimesSync(file, date, date);
    const result = run('bash', [join(root, 'scripts/check-tree-quiescent.sh'), mode === 'invalid' ? '0' : '90'], { env: { ...process.env, PATH: `${join(root, 'bin')}:${process.env.PATH}` } });
    assert.equal(result.status, { old: 0, recent: 1, invalid: 2 }[mode], result.stdout + result.stderr);
    if (mode === 'recent') assert.match(result.stdout, /\d{4}-\d{2}-\d{2}.*ui\/src\/space name.ts/);
  });
}
