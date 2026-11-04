// QA helper: spawns one local pane in an isolated daemon and prints `SESSION_ID=<id>`.
//
//   bun scripts/qa/spawn-pane.mjs --runtime <FERRYX_RUNTIME_DIR> [--cwd <dir>] [--shell <path>]
//
// Speaks the daemon data protocol v5 (JSON lines, src-tauri/src/daemon/protocol.rs DaemonRequest):
// handshake, registerWorkspace, spawn. Unix connects to <runtime>/daemon.sock; Windows reads
// <runtime>/daemon.port and daemon.token and sends the token on the first frame only. The pane
// outlives this process: the daemon owns the PTY. Exits 1 with the daemon's reply on any failure.

import net from 'node:net';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { once } from 'node:events';

const PROTOCOL_VERSION = 5;
const REPLY_TIMEOUT_MS = 15000;

function parseArgs(argv) {
  const options = { runtime: null, cwd: process.cwd(), shell: null };
  for (let i = 0; i < argv.length; i += 1) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (!['--runtime', '--cwd', '--shell'].includes(flag) || value === undefined) {
      throw new Error(`unknown or incomplete argument: ${flag}`);
    }
    options[flag.slice(2)] = value;
    i += 1;
  }
  if (!options.runtime) throw new Error('--runtime <dir> is required');
  return options;
}

async function connect(runtime) {
  if (process.platform !== 'win32') {
    return { socket: net.createConnection({ path: join(runtime, 'daemon.sock') }), token: null };
  }
  const port = Number((await readFile(join(runtime, 'daemon.port'), 'utf8')).trim());
  const token = (await readFile(join(runtime, 'daemon.token'), 'utf8')).trim();
  return { socket: net.createConnection({ host: '127.0.0.1', port }), token };
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseArgs(argv);
  const { socket, token: firstFrameToken } = await connect(options.runtime);
  await once(socket, 'connect', { signal: AbortSignal.timeout(REPLY_TIMEOUT_MS) });
  const lines = createInterface({ input: socket });
  let token = firstFrameToken;
  const call = async (request) => {
    const reply = once(lines, 'line', { signal: AbortSignal.timeout(REPLY_TIMEOUT_MS) });
    const frame = token ? { ...request, token } : request;
    token = null;
    socket.write(`${JSON.stringify(frame)}\n`);
    return JSON.parse((await reply)[0]);
  };
  const expect = (reply, type) => {
    if (reply?.type !== type) throw new Error(`expected ${type}, got ${JSON.stringify(reply)}`);
    return reply;
  };
  try {
    expect(await call({ type: 'handshake', version: PROTOCOL_VERSION }), 'handshakeOk');
    expect(
      await call({ type: 'registerWorkspace', workspaceId: 'qa-spawn-pane', repoRoot: options.cwd }),
      'registerWorkspaceOk',
    );
    const spawn = {
      type: 'spawn',
      clientRequestId: `qa-spawn-pane-${process.pid}`,
      workspaceId: 'qa-spawn-pane',
      worktree: null,
      cwd: options.cwd,
      cols: 80,
      rows: 24,
    };
    if (options.shell) spawn.shell = options.shell;
    const spawned = expect(await call(spawn), 'spawnOk');
    console.log(`SESSION_ID=${spawned.sessionId}`);
  } finally {
    lines.close();
    socket.end();
  }
}

if (import.meta.main) {
  main().catch((error) => {
    console.error(`SPAWN_PANE_FAILED: ${error.message}`);
    process.exit(1);
  });
}
