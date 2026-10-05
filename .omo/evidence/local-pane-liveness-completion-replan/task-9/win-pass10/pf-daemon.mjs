// VERIFIER SCRATCH — writes the daemon's own answer to a FILE (no stdout capture).
import { readFileSync, writeFileSync } from 'node:fs';
import { connect } from 'node:net';
import { join } from 'node:path';

const runtimeDir = process.argv[2];
const command = process.argv[3] || 'list';
const outFile = process.argv[4];
const sessionId = process.argv[5];

const out = { runtimeDir, command, port: null, handshake: null, response: null, error: null };
function finish() { try { writeFileSync(outFile, JSON.stringify(out, null, 2)); } catch {} process.exit(0); }

try {
  const port = Number(readFileSync(join(runtimeDir, 'daemon.port'), 'utf8').trim());
  out.port = port;
  const token = readFileSync(join(runtimeDir, 'daemon.token'), 'utf8').trim();
  const socket = connect({ host: '127.0.0.1', port });
  let buffer = '';
  const lines = [];
  socket.on('connect', () => socket.write(JSON.stringify({ type: 'handshake', version: 5, token }) + '\n'));
  socket.on('data', chunk => {
    buffer += chunk.toString('utf8');
    let idx;
    while ((idx = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, idx).trim();
      buffer = buffer.slice(idx + 1);
      if (!line) continue;
      lines.push(line);
      if (lines.length === 1) {
        try { out.handshake = JSON.parse(line); } catch { out.handshake = line; }
        socket.write(JSON.stringify(command === 'describe' ? { type: 'describeSession', sessionId } : { type: 'listSessions' }) + '\n');
      } else if (lines.length === 2) {
        try { out.response = JSON.parse(line); } catch { out.response = line; }
        socket.end();
      }
    }
  });
  socket.on('error', err => { out.error = String(err && err.message ? err.message : err); finish(); });
  socket.on('close', finish);
  setTimeout(() => { out.error = out.error || 'timeout'; try { socket.destroy(); } catch {} finish(); }, 6000);
} catch (error) {
  out.error = String(error && error.message ? error.message : error);
  finish();
}
