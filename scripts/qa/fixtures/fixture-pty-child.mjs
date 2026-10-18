import { createServer } from 'node:net';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { StringDecoder } from 'node:string_decoder';

/**
 * Retained PTY Fixture Child Process
 * Spawned ONCE by Ferryx daemon as the initial PTY root shell via FERRYX_QA_SHELL.
 * Owns an event-driven control IPC and emits markers directly to stdout.
 * Zero polling, zero writeTerminal input generation, zero replacement spawns.
 */
async function main() {
  const profile = process.env.FERRYX_QA_PROFILE
    || (process.env.FERRYX_DATA_DIR ? join(process.env.FERRYX_DATA_DIR, '..') : process.cwd());

  const expectedRunId = process.env.FERRYX_QA_RUN_ID || null;
  const expectedOperationId = process.env.FERRYX_QA_OPERATION_ID || null;

  const isWindows = process.platform === 'win32';
  const controlSocketPath = join(profile, 'fixture-control.sock');
  let controlEndpoint;

  const decoder = new StringDecoder('utf8');
  let buf = '';

  const server = createServer((socket) => {
    socket.on('data', (chunk) => {
      buf += decoder.write(chunk);
      let newlineIdx;
      while ((newlineIdx = buf.indexOf('\n')) !== -1) {
        const line = buf.slice(0, newlineIdx);
        buf = buf.slice(newlineIdx + 1);
        if (!line.trim()) continue;

        let req;
        try {
          req = JSON.parse(line);
        } catch (_) {
          socket.write(JSON.stringify({ type: 'error', error: 'MALFORMED_JSON' }) + '\n');
          continue;
        }

        if (req.type === 'emit-marker') {
          if (expectedRunId && req.runId !== expectedRunId) {
            socket.write(JSON.stringify({ type: 'error', code: 'SCOPE_MISMATCH', message: 'runId mismatch' }) + '\n');
            continue;
          }
          if (expectedOperationId && req.operationId !== expectedOperationId) {
            socket.write(JSON.stringify({ type: 'error', code: 'SCOPE_MISMATCH', message: 'operationId mismatch' }) + '\n');
            continue;
          }
          if (typeof req.marker !== 'string' || !req.marker) {
            socket.write(JSON.stringify({ type: 'error', code: 'MARKER_REQUIRED' }) + '\n');
            continue;
          }

          // Emit marker directly to PTY stdout
          process.stdout.write(`\r\n  ${req.marker}\r\n  ${req.marker}\r\n`);

          // Return delivery ack with scoped correlation
          socket.write(JSON.stringify({
            type: 'marker-emitted',
            marker: req.marker,
            runId: req.runId ?? expectedRunId,
            operationId: req.operationId ?? expectedOperationId,
            pid: process.pid,
            timestamp: Date.now(),
          }) + '\n');
        } else if (req.type === 'ping') {
          socket.write(JSON.stringify({ type: 'pong', pid: process.pid }) + '\n');
        }
      }
    });
  });

  if (isWindows) {
    await new Promise((res, rej) => {
      server.listen(0, '127.0.0.1', res);
      server.once('error', rej);
    });
    const addr = server.address();
    controlEndpoint = { host: '127.0.0.1', port: addr.port };
    await writeFile(join(profile, 'fixture-control.port'), String(addr.port), 'utf8');
  } else {
    await new Promise((res, rej) => {
      server.listen(controlSocketPath, res);
      server.once('error', rej);
    });
    controlEndpoint = { path: controlSocketPath };
  }

  // Publish fixture readiness receipt - readiness walltime ONLY, not OS process start
  const readyReceipt = {
    status: 'ready',
    pid: process.pid,
    ppid: process.ppid,
    readyTimestamp: Date.now(),
    endpoint: controlEndpoint,
    runId: expectedRunId,
    operationId: expectedOperationId,
  };
  await writeFile(
    join(profile, 'receipts', 'fixture-child-ready.json'),
    JSON.stringify(readyReceipt, null, 2),
    'utf8'
  );

  // Keep alive indefinitely until terminated by parent daemon or OS signal
  process.on('SIGTERM', () => {
    server.close();
    process.exit(0);
  });
  process.on('SIGINT', () => {});
}

main().catch((err) => {
  process.stderr.write(`fixture-pty-child error: ${err.message}\n`);
  process.exit(1);
});
