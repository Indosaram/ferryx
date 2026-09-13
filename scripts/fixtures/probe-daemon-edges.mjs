// Portable staging fixture only. It never connects to or starts a daemon.
import { realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
process.stdout.write(JSON.stringify({ driver: realpathSync(fileURLToPath(import.meta.url)), nonce: process.argv[2] }));
