// No Cargo execution without the lead's explicit exclusive-slot receipt.
import { existsSync, mkdtempSync, mkdirSync, openSync, closeSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const [phase, target, sharedArgument, slotArgument] = process.argv.slice(2);
if (!['red', 'green'].includes(phase) || !['debug', 'input'].includes(target) || !sharedArgument || !slotArgument) {
  throw new Error('Requires phase red|green, target debug|input, provisioned shared Cargo root and lead exclusive-slot receipt');
}
const shared = resolve(sharedArgument);
const slot = resolve(slotArgument);
if (!existsSync(slot) || !existsSync(join(shared, 'cargo')) || !existsSync(join(shared, 'target'))) {
  throw new Error('Lead slot receipt and provisioned Cargo directories must exist');
}
const cwd = resolve(import.meta.dir, '../../..');
const root = mkdtempSync(join(tmpdir(), 'ferryx-p02-st_01a099ff-'));
for (const name of ['home', 'profile', 'appdata', 'localappdata', 'runtime', 'data', 'sessions', 'tmp', 'config', 'cache']) mkdirSync(join(root, name));
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
const command = [join(toolchain, 'cargo'), 'test', '--manifest-path', 'src-tauri/Cargo.toml',
  ...(target === 'debug' ? ['--lib', 'ipc::debug::'] : ['--test', 'native_terminal_input_boundary_contract']), '--', '--nocapture'];
const hashes = {};
for (const file of ['src-tauri/src/ipc/debug.rs', 'src-tauri/src/ipc/native_terminal.rs', 'src-tauri/tests/native_terminal_input_boundary_contract.rs', 'src-tauri/Cargo.lock']) {
  hashes[file] = new Bun.CryptoHasher('sha256').update(await Bun.file(join(cwd, file)).arrayBuffer()).digest('hex');
}
const receipt = { phase, target, root, cwd, slot, command, env, policy, hashes, started: new Date().toISOString() };
writeFileSync(join(root, 'started.json'), JSON.stringify(receipt, null, 2), { flag: 'wx' });
const fd = openSync(join(root, `${phase}-${target}.log`), 'wx');
try {
  const child = Bun.spawn(['/usr/bin/sandbox-exec', '-p', policy, ...command], { cwd, env, stdin: 'ignore', stdout: fd, stderr: fd });
  console.log(JSON.stringify({ root, pid: child.pid, command }));
  const exit = await child.exited;
  writeFileSync(join(root, 'receipt.json'), JSON.stringify({ ...receipt, pid: child.pid, exit, finished: new Date().toISOString() }, null, 2), { flag: 'wx' });
  process.exitCode = exit;
} finally {
  closeSync(fd);
}
// Retain owned evidence root for the lead; never clean the borrowed Cargo target/cache.
