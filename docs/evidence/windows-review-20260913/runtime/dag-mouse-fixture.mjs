// Run inside the owned visible PTY, not via SSH. Raw bytes are the oracle.
import { appendFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
const output = resolve(process.argv[2] ?? '');
const expected = 'C:\\Users\\sook\\ferryx-qa-dag-st01a099f8\\evidence';
if (process.platform !== 'win32' || dirname(output) !== expected || !process.stdin.isTTY) {
  throw new Error('Requires interactive owned Windows PTY and output in allocated evidence directory');
}
appendFileSync(output, JSON.stringify({ event: 'start', pid: process.pid, cwd: process.cwd() }) + '\n', { flag: 'ax' });
let closed = false;
function cleanup() {
  if (closed) return;
  closed = true;
  process.stdout.write('\x1b[?1006l\x1b[?1000l\x1b[?1049l');
  process.stdin.setRawMode(false);
  process.stdin.pause();
}
process.on('exit', cleanup);
process.stdin.on('error', error => { cleanup(); throw error; });
process.stdin.on('data', bytes => {
  appendFileSync(output, JSON.stringify({ event: 'input', hex: bytes.toString('hex') }) + '\n');
  if (bytes.includes(3) || bytes.includes(113)) cleanup();
});
process.stdin.setRawMode(true);
process.stdin.resume();
process.stdout.write('\x1b[?1049h\x1b[?1000h\x1b[?1006hDAG_MOUSE_READY\r\n');
