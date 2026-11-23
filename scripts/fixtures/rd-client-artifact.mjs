import { createHash } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { deflateRawSync } from 'node:zlib';

/**
 * Writes one member of a zip archive into a local-file-header + data + central-directory layout.
 * Release verification reads real archives (`listZipEntries`), so the fixtures must be real zips
 * rather than files that merely end in `.zip`.
 */
function zipMember(name, body) {
  const nameBytes = Buffer.from(name, 'utf8');
  const compressed = deflateRawSync(body);
  const crc = crc32(body);
  const local = Buffer.alloc(30);
  local.writeUInt32LE(0x04034b50, 0);
  local.writeUInt16LE(20, 4); // version needed
  local.writeUInt16LE(0, 6); // flags
  local.writeUInt16LE(8, 8); // deflate
  local.writeUInt16LE(0, 10); // mod time
  local.writeUInt16LE(0, 12); // mod date
  local.writeUInt32LE(crc, 14);
  local.writeUInt32LE(compressed.length, 18);
  local.writeUInt32LE(body.length, 22);
  local.writeUInt16LE(nameBytes.length, 26);
  local.writeUInt16LE(0, 28);
  return { nameBytes, compressed, crc, local };
}

const CRC_TABLE = (() => {
  const table = new Int32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c;
  }
  return table;
})();

function crc32(bytes) {
  let crc = -1;
  for (const byte of bytes) crc = CRC_TABLE[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  return (crc ^ -1) >>> 0;
}

/** Builds a stored-deflate zip archive from `[{ name, body }]`. */
export function buildZip(entries) {
  const parts = [];
  const central = [];
  let offset = 0;
  for (const { name, body } of entries) {
    const member = zipMember(name, Buffer.from(body));
    parts.push(member.local, member.nameBytes, member.compressed);
    const header = Buffer.alloc(46);
    header.writeUInt32LE(0x02014b50, 0);
    header.writeUInt16LE(20, 4); // version made by
    header.writeUInt16LE(20, 6); // version needed
    header.writeUInt16LE(0, 8); // flags
    header.writeUInt16LE(8, 10); // deflate
    header.writeUInt16LE(0, 12);
    header.writeUInt16LE(0, 14);
    header.writeUInt32LE(member.crc, 16);
    header.writeUInt32LE(member.compressed.length, 20);
    header.writeUInt32LE(body.length, 24);
    header.writeUInt16LE(member.nameBytes.length, 28);
    header.writeUInt16LE(0, 30); // extra
    header.writeUInt16LE(0, 32); // comment
    header.writeUInt16LE(0, 34); // disk
    header.writeUInt16LE(0, 36); // internal attrs
    header.writeUInt32LE(0, 38); // external attrs
    header.writeUInt32LE(offset, 42);
    central.push(header, member.nameBytes);
    offset += member.local.length + member.nameBytes.length + member.compressed.length;
  }
  const centralBytes = Buffer.concat(central);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(0, 4);
  eocd.writeUInt16LE(0, 6);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(centralBytes.length, 12);
  eocd.writeUInt32LE(offset, 16);
  eocd.writeUInt16LE(0, 20);
  return Buffer.concat([...parts, centralBytes, eocd]);
}

/**
 * Writes the artifact a host publishes for the remote-desktop client. Windows publishes one zip
 * carrying the client and the FFmpeg DLLs it loads, because a bare .exe cannot start without them.
 */
export function writeRdClientArtifact(dir, platform) {
  const [kind, name] = {
    darwin: ['rd-client-darwin-universal', 'ferryx-rd-client-darwin-universal'],
    linux: ['rd-client-linux-amd64', 'ferryx-rd-client-linux-amd64'],
    windows: ['rd-client-windows-amd64', 'ferryx-rd-client-windows-amd64.zip'],
  }[platform];
  const bytes = platform === 'windows'
    ? buildZip([
      { name: 'ferryx-rd-client.exe', body: `fixture: ${kind}\n` },
      { name: 'avcodec-63.dll', body: 'fixture: avcodec\n' },
      { name: 'avutil-61.dll', body: 'fixture: avutil\n' },
      { name: 'swscale-10.dll', body: 'fixture: swscale\n' },
      { name: 'swresample-7.dll', body: 'fixture: swresample\n' },
    ])
    : Buffer.from(`fixture: ${kind}\n`);
  writeFileSync(join(dir, name), bytes);
  return {
    kind, name, relPath: `${platform}/${name}`, bytes: bytes.length,
    sha256: createHash('sha256').update(bytes).digest('hex'), signatureRelPath: null, targets: [],
  };
}
