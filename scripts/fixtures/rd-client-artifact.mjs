import { createHash } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

export function writeRdClientArtifact(dir, platform) {
  const [kind, name] = {
    darwin: ['rd-client-darwin-universal', 'ferryx-rd-client-darwin-universal'],
    linux: ['rd-client-linux-amd64', 'ferryx-rd-client-linux-amd64'],
    windows: ['rd-client-windows-amd64', 'ferryx-rd-client-windows-amd64.exe'],
  }[platform];
  const bytes = Buffer.from(`fixture: ${kind}\n`);
  writeFileSync(join(dir, name), bytes);
  return {
    kind, name, relPath: `${platform}/${name}`, bytes: bytes.length,
    sha256: createHash('sha256').update(bytes).digest('hex'), signatureRelPath: null, targets: [],
  };
}
