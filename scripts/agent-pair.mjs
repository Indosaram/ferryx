#!/usr/bin/env bun
import net from 'node:net';
import path from 'node:path';

const pin = process.argv[2];
const displayLabel = process.argv[3] || 'omaki';
const relayOrigin = process.argv[4] || 'https://relay.checka.cc';

if (!pin) {
    console.error('Usage: bun scripts/agent-pair.mjs <pin> [displayLabel] [relayOrigin]');
    process.exit(1);
}

// Resolves the live daemon endpoint the way src-tauri/src/daemon/server.rs::get_runtime_dir does:
// FERRYX_RUNTIME_DIR wins, then /tmp/rorca-<uid>{-dev} on unix or %LOCALAPPDATA%\Ferryx\runtime{,-dev}
// on Windows. Fails with a clear message instead of a TypeError where neither applies.
function daemonSocketPath(dev = false) {
    const override = process.env.FERRYX_RUNTIME_DIR;
    if (override) return path.join(override, process.platform === 'win32' ? 'daemon.port' : 'daemon.sock');
    if (process.platform === 'win32') {
        const base = process.env.LOCALAPPDATA || process.env.TEMP || 'C:\\ProgramData';
        return path.join(base, 'Ferryx', dev ? 'runtime-dev' : 'runtime', 'daemon.port');
    }
    if (typeof process.getuid !== 'function') {
        console.error(`Cannot resolve the Ferryx daemon runtime dir on ${process.platform}: expected /tmp/rorca-<uid> on unix or %LOCALAPPDATA%\\Ferryx\\runtime on Windows; set FERRYX_RUNTIME_DIR to override.`);
        process.exit(2);
    }
    return path.join('/tmp', `rorca-${process.getuid()}${dev ? '-dev' : ''}`, 'daemon.sock');
}

// Check dev socket first, fallback to standard socket
const devSocket = daemonSocketPath(true);
const stdSocket = daemonSocketPath(false);

import fs from 'node:fs';
const socketPath = fs.existsSync(devSocket) ? devSocket : stdSocket;
console.log(`Connecting to daemon socket: ${socketPath}...`);

const sock = net.connect(socketPath);
let buffer = '';

sock.on('connect', () => {
    sock.write(JSON.stringify({ type: 'handshake', version: 3 }) + '\n');
});

sock.on('data', (data) => {
    buffer += data.toString();
    const lines = buffer.split('\n');
    buffer = lines.pop();
    for (const line of lines) {
        if (!line.trim()) continue;
        const msg = JSON.parse(line);
        if (msg.type === 'handshakeOk') {
            console.log(`Connected to daemon (PID ${msg.pid}, version ${msg.version}).`);
            console.log(`Sending pairedHostPair request with PIN ${pin}...`);
            sock.write(JSON.stringify({
                type: 'pairedHostPair',
                request: { relayOrigin, pin, displayLabel }
            }) + '\n');
        } else if (msg.type === 'pairedHostPairOk') {
            console.log('Pairing successful!');
            console.log(JSON.stringify(msg.host, null, 2));
            sock.end();
            process.exit(0);
        } else if (msg.type === 'pairedHostError' || msg.type === 'error') {
            console.error('Pairing failed:', JSON.stringify(msg.error || msg, null, 2));
            sock.end();
            process.exit(1);
        }
    }
});

sock.on('error', (err) => {
    console.error('Socket error:', err.message);
    process.exit(1);
});
