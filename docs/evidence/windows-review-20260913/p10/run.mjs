// Lead invokes only after granting the shared Darwin Cargo slot.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join, isAbsolute } from 'node:path';
import { createHash } from 'node:crypto';

const [phase, cargo, cargoHome, rustupHome, target] = process.argv.slice(2);
if (!['red', 'green'].includes(phase) || ![cargo, cargoHome, rustupHome, target].every(p => p && isAbsolute(p))) {
  throw new Error('Usage: node run.mjs red|green ABS_CARGO ABS_CARGO_HOME ABS_RUSTUP_HOME ABS_LEAD_TARGET; exclusive lead slot required');
}
const cwd = process.cwd();
const evidence = resolve('docs/evidence/windows-review-20260913/p10');
const root = mkdtempSync(join(tmpdir(), 'ferryx-p10-st_01a09a03-'));
const env = { ...process.env, CARGO_HOME: cargoHome, RUSTUP_HOME: rustupHome, CARGO_TARGET_DIR: target, CARGO_BUILD_JOBS: '2' };
for (const [key, leaf] of Object.entries({ HOME: 'home', USERPROFILE: 'home', APPDATA: 'appdata', LOCALAPPDATA: 'localappdata', FERRYX_RUNTIME_DIR: 'runtime', FERRYX_DATA_DIR: 'data', FERRYX_SESSION_DIR: 'sessions', XDG_CONFIG_HOME: 'config', XDG_CACHE_HOME: 'cache', XDG_DATA_HOME: 'xdgdata', TMPDIR: 'tmp', TMP: 'tmp', TEMP: 'tmp' })) {
  env[key] = join(root, leaf);
  mkdirSync(env[key], { recursive: true });
}
env.GIT_CONFIG_NOSYSTEM = '1';
env.GIT_CONFIG_GLOBAL = join(root, 'empty-gitconfig');
writeFileSync(env.GIT_CONFIG_GLOBAL, '');
env.RUSTC_WRAPPER = '';
env.ZIG = '/opt/homebrew/bin/zig';
env.PATH = '/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin';
env.CARGO_NET_OFFLINE = 'true';
env.CARGO_BUILD_JOBS = '8';
env.CARGO_TERM_COLOR = 'never';
const commands = [
  ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--lib', 'ssh::config::tests::p10_', '--', '--nocapture'],
  ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--test', 'worktree_safety', 'p10_namespace_accepts_nested_unicode_and_non_devices', '--', '--exact', '--nocapture'],
];
let failed = false;
try {
  writeFileSync(join(evidence, `${phase}.started.json`), JSON.stringify({ cwd, root, cargo, cargoHome, rustupHome, target, commands, configSha256: createHash('sha256').update(readFileSync('src-tauri/src/ssh/config.rs')).digest('hex') }, null, 2));
  for (const [index, args] of commands.entries()) {
    const result = spawnSync(cargo, args, { cwd, env, encoding: 'utf8', timeout: 300000, maxBuffer: 16 * 1024 * 1024 });
    const output = `${result.stdout ?? ''}\n${result.stderr ?? ''}`;
    writeFileSync(join(evidence, `${phase}.${index}.log`), output);
    writeFileSync(join(evidence, `${phase}.${index}.json`), JSON.stringify({ command: [cargo, ...args], status: result.status, signal: result.signal, error: result.error?.message }, null, 2));
    console.log(JSON.stringify({ index, status: result.status, signal: result.signal }));
    if (result.error || result.signal) throw result.error ?? new Error(`Cargo terminated: ${result.signal}; inspect children before reusing slot`);
    if (result.status !== 0) failed = true;
  }
} finally {
  rmSync(root, { recursive: true });
  writeFileSync(join(evidence, `${phase}.cleanup.json`), JSON.stringify({ removedOwnedRoot: root, touchedDaemon: false, nativeExecution: false }, null, 2));
}
process.exitCode = failed ? 1 : 0;
