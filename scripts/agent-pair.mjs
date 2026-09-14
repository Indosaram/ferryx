#!/usr/bin/env bun
import net from 'node:net';

const pin = process.argv[2];
const displayLabel = process.argv[3] || 'omaki';
const relayOrigin = process.argv[4] || 'https://relay.checka.cc';

if (!pin) {
    console.error('Usage: bun scripts/agent-pair.mjs <pin> [displayLabel] [relayOrigin]');
    process.exit(1);
}

// Check dev socket first, fallback to standard socket
const devSocket = `/tmp/rorca-${process.getuid()}-dev/daemon.sock`;
const stdSocket = `/tmp/rorca-${process.getuid()}/daemon.sock`;

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
