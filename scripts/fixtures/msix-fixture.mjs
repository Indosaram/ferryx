import { deflateRawSync } from "node:zlib";

/** Builds a minimal one-entry deflated zip, enough to stand in for an MSIX package. */
export function makeZip(entryName, content) {
  const name = Buffer.from(entryName, "utf8");
  const raw = Buffer.from(content, "utf8");
  const data = deflateRawSync(raw);
  const local = Buffer.alloc(30);
  local.writeUInt32LE(0x04034b50, 0);
  local.writeUInt16LE(20, 4);
  local.writeUInt16LE(8, 8);
  local.writeUInt32LE(data.length, 18);
  local.writeUInt32LE(raw.length, 22);
  local.writeUInt16LE(name.length, 26);
  const central = Buffer.alloc(46);
  central.writeUInt32LE(0x02014b50, 0);
  central.writeUInt16LE(20, 4);
  central.writeUInt16LE(20, 6);
  central.writeUInt16LE(8, 10);
  central.writeUInt32LE(data.length, 20);
  central.writeUInt32LE(raw.length, 24);
  central.writeUInt16LE(name.length, 28);
  const centralOffset = local.length + name.length + data.length;
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(1, 8);
  eocd.writeUInt16LE(1, 10);
  eocd.writeUInt32LE(central.length + name.length, 12);
  eocd.writeUInt32LE(centralOffset, 16);
  return Buffer.concat([local, name, data, central, name, eocd]);
}

/** MSIX bytes whose AppxManifest.xml Identity carries `version`. */
export function msixWithVersion(version) {
  return makeZip(
    "AppxManifest.xml",
    `<?xml version="1.0"?><Package><Identity Name="ProjectMaho.Ferryx" Publisher="CN=x" Version="${version}" ProcessorArchitecture="x64" /></Package>`,
  );
}
