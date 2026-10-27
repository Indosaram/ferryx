#!/usr/bin/env node
// Capability-filter fixture for the `runtime` scenario. Runs the real mhc with the arguments it
// was given and relays stdio unchanged, except that the `get_protocol_info` reply loses the one
// capability named by UNIFIED_SUPERVISOR_MHC_DROP_CAPABILITY. The supervisor must then refuse the
// engine as MHC_CAPABILITY_MISSING before opening any session.
//   UNIFIED_SUPERVISOR_MHC_REAL         absolute path of the real mhc binary (required)
//   UNIFIED_SUPERVISOR_MHC_DROP_CAPABILITY  the capability to remove (required)
//   UNIFIED_SUPERVISOR_MHC_FRAMES       optional JSONL file: every stdin line received is appended
//                                       (`{type,id}` for JSON, `{unparsed:<line>}` otherwise)
import { spawn } from 'node:child_process';
import { appendFileSync } from 'node:fs';

const real = process.env.UNIFIED_SUPERVISOR_MHC_REAL;
const drop = process.env.UNIFIED_SUPERVISOR_MHC_DROP_CAPABILITY;
const frames = process.env.UNIFIED_SUPERVISOR_MHC_FRAMES;
if (!real || !drop) {
  process.stderr.write('fixture-mhc-filter: UNIFIED_SUPERVISOR_MHC_REAL and UNIFIED_SUPERVISOR_MHC_DROP_CAPABILITY are required\n');
  process.exit(2);
}

const child = spawn(real, process.argv.slice(2), { stdio: ['pipe', 'pipe', 'inherit'] });
child.on('error', (error) => { process.stderr.write(`fixture-mhc-filter: ${error?.message ?? error}\n`); process.exit(127); });
child.on('exit', (code, signal) => process.exit(code ?? (signal ? 128 : 1)));
for (const name of ['SIGTERM', 'SIGINT', 'SIGHUP']) process.on(name, () => child.kill(name));

// Command ids of get_protocol_info requests, so only their responses are rewritten.
const protocolIds = new Set();

function lines(stream, onLine, onEnd) {
  let buffer = '';
  stream.setEncoding('utf8');
  stream.on('data', (chunk) => {
    buffer += chunk;
    for (let index = buffer.indexOf('\n'); index >= 0; index = buffer.indexOf('\n')) {
      onLine(buffer.slice(0, index));
      buffer = buffer.slice(index + 1);
    }
  });
  stream.on('end', () => { if (buffer) onLine(buffer); onEnd(); });
}

lines(process.stdin, (line) => {
  let record;
  try {
    const command = JSON.parse(line);
    record = { type: command?.type ?? null, id: command?.id ?? null };
    if (command?.type === 'get_protocol_info' && command.id !== undefined) protocolIds.add(String(command.id));
  } catch {
    // Relayed unchanged, and recorded: a non-JSON write is still a send.
    record = { unparsed: line };
  }
  if (frames) appendFileSync(frames, `${JSON.stringify(record)}\n`);
  child.stdin.write(`${line}\n`);
}, () => child.stdin.end());

lines(child.stdout, (line) => {
  let out = line;
  try {
    const record = JSON.parse(line);
    if (record?.type === 'response' && protocolIds.has(String(record.id)) && Array.isArray(record.data?.capabilities)) {
      protocolIds.delete(String(record.id));
      record.data.capabilities = record.data.capabilities.filter((name) => name !== drop);
      out = JSON.stringify(record);
    }
  } catch { /* relayed unchanged */ }
  process.stdout.write(`${out}\n`);
}, () => {});
