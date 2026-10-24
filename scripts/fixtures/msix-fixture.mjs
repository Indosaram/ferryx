import { deflateRawSync } from "node:zlib";

/**
 * Builds a minimal one-entry deflated zip, enough to stand in for an MSIX package. With
 * `zip64: true` it mirrors MakeAppx output: saturated sizes/offsets in the central directory
 * resolved through a 0x0001 extra field, plus a ZIP64 EOCD record and locator with a
 * saturated classic EOCD.
 */
export function makeZip(entryName, content, { zip64 = false } = {}) {
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
  const extra = Buffer.alloc(zip64 ? 28 : 0);
  if (zip64) {
    extra.writeUInt16LE(0x0001, 0);
    extra.writeUInt16LE(24, 2);
    extra.writeBigUInt64LE(BigInt(raw.length), 4);
    extra.writeBigUInt64LE(BigInt(data.length), 12);
    extra.writeBigUInt64LE(0n, 20);
  }
  const central = Buffer.alloc(46);
  central.writeUInt32LE(0x02014b50, 0);
  central.writeUInt16LE(zip64 ? 45 : 20, 4);
  central.writeUInt16LE(zip64 ? 45 : 20, 6);
  central.writeUInt16LE(8, 10);
  central.writeUInt32LE(zip64 ? 0xffffffff : data.length, 20);
  central.writeUInt32LE(zip64 ? 0xffffffff : raw.length, 24);
  central.writeUInt16LE(name.length, 28);
  central.writeUInt16LE(extra.length, 30);
  central.writeUInt32LE(zip64 ? 0xffffffff : 0, 42);
  const centralOffset = local.length + name.length + data.length;
  const centralSize = central.length + name.length + extra.length;
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(zip64 ? 0xffff : 1, 8);
  eocd.writeUInt16LE(zip64 ? 0xffff : 1, 10);
  eocd.writeUInt32LE(zip64 ? 0xffffffff : centralSize, 12);
  eocd.writeUInt32LE(zip64 ? 0xffffffff : centralOffset, 16);
  if (!zip64) return Buffer.concat([local, name, data, central, name, eocd]);
  const record = Buffer.alloc(56);
  record.writeUInt32LE(0x06064b50, 0);
  record.writeBigUInt64LE(44n, 4);
  record.writeUInt16LE(45, 12);
  record.writeUInt16LE(45, 14);
  record.writeBigUInt64LE(1n, 24);
  record.writeBigUInt64LE(1n, 32);
  record.writeBigUInt64LE(BigInt(centralSize), 40);
  record.writeBigUInt64LE(BigInt(centralOffset), 48);
  const locator = Buffer.alloc(20);
  locator.writeUInt32LE(0x07064b50, 0);
  locator.writeBigUInt64LE(BigInt(centralOffset + centralSize), 8);
  locator.writeUInt32LE(1, 16);
  return Buffer.concat([local, name, data, central, name, extra, record, locator, eocd]);
}

/** MSIX bytes whose AppxManifest.xml Identity carries `version`. */
export function msixWithVersion(version, options = {}) {
  return makeZip(
    "AppxManifest.xml",
    `<?xml version="1.0"?><Package><Identity Name="ProjectMaho.Ferryx" Publisher="CN=x" Version="${version}" ProcessorArchitecture="x64" /></Package>`,
    options,
  );
}
