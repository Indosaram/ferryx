import { mkdtempSync, mkdirSync, openSync, closeSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const [phase, shared, slot] = process.argv.slice(2);
if (!['red', 'green'].includes(phase) || !shared || !slot) {
  throw new Error('Usage: bun p08-runner.mjs red|green ABSOLUTE_LEAD_SHARED_ROOT LEAD_SLOT_RECEIPT');
}
if (!existsSync(slot) || !existsSync(join(shared, 'cargo')) || !existsSync(join(shared, 'target'))) {
  throw new Error('Lead slot receipt and provisioned shared Cargo directories are required');
}
const cwd = resolve(import.meta.dir, '../../..');
const root = mkdtempSync(join(tmpdir(), 'ferryx-p08-st_01a09a01-'));
for (const dir of ['home', 'profile', 'appdata', 'localappdata', 'runtime', 'data', 'sessions', 'tmp', 'config', 'cache']) mkdirSync(join(root, dir));
const toolchain = '/Users/indo/.rustup/toolchains/1.98.1-aarch64-apple-darwin/bin';

const env = {
  HOME: join(root, 'home'), USERPROFILE: join(root, 'profile'),
  APPDATA: join(root, 'appdata'), LOCALAPPDATA: join(root, 'localappdata'),
  FERRYX_RUNTIME_DIR: join(root, 'runtime'), FERRYX_DATA_DIR: join(root, 'data'), FERRYX_SESSION_DIR: join(root, 'sessions'),
  XDG_CONFIG_HOME: join(root, 'config'), XDG_CACHE_HOME: join(root, 'cache'), XDG_DATA_HOME: join(root, 'data'), XDG_RUNTIME_DIR: join(root, 'runtime'),
  TMPDIR: join(root, 'tmp'), TMP: join(root, 'tmp'), TEMP: join(root, 'tmp'),
  CARGO_TARGET_DIR: join(shared, 'target'), CARGO_HOME: join(shared, 'cargo'), CARGO_NET_OFFLINE: 'true', CARGO_BUILD_JOBS: '8', CARGO_TERM_COLOR: 'never',
  RUSTC: join(toolchain, 'rustc'), RUSTDOC: join(toolchain, 'rustdoc'), RUSTC_WRAPPER: '',
  ZIG: '/opt/homebrew/bin/zig', PATH: `${toolchain}:/usr/bin:/bin:/usr/sbin:/sbin`,
  GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', LANG: 'en_US.UTF-8',
};
const policy = `(version 1)(allow default)(deny network*)(deny file-write* (subpath "${cwd}") (subpath "/Users/indo/.cargo") (subpath "/Users/indo/.rustup") (subpath "/Users/indo/.cache") (subpath "/Users/indo/Library/Caches"))`;

const tests = [
  'daemon::client::tests::p08_upgrade_rpc_inherits_admission',
  'daemon::server::tests::p08_remote_cwd_aliases_preserve_relative_path',
];

const hashes = {};
for (const file of ['src-tauri/src/daemon/client.rs', 'src-tauri/src/daemon/server.rs', 'src-tauri/Cargo.lock']) {
  hashes[file] = new Bun.CryptoHasher('sha256').update(await Bun.file(join(cwd, file)).arrayBuffer()).digest('hex');
}
const receipt = { phase, root, cwd, slot: resolve(slot), tests, env, policy, hashes, started: new Date().toISOString() };
writeFileSync(join(root, 'started.json'), JSON.stringify(receipt, null, 2), { flag: 'wx' });

let totalExit = 0;
const fd = openSync(join(root, `${phase}.log`), 'wx');
try {
  for (const testName of tests) {
    const command = [join(toolchain, 'cargo'), 'test', '--manifest-path', 'src-tauri/Cargo.toml', '--lib', testName, '--', '--exact', '--nocapture'];
    console.log(JSON.stringify({ testName, phase }));
    const child = Bun.spawn(['/usr/bin/sandbox-exec', '-p', policy, ...command], { cwd, env, stdin: 'ignore', stdout: fd, stderr: fd });
    const exit = await child.exited;
    if (exit !== 0) totalExit = exit;
  }
  writeFileSync(join(root, 'receipt.json'), JSON.stringify({ ...receipt, totalExit, finished: new Date().toISOString() }, null, 2), { flag: 'wx' });
  process.exitCode = totalExit;
} finally {
  closeSync(fd);
}
