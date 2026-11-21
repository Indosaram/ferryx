// The Mach-O floor reader, tested on bytes it can be given deterministically.
//
// The real consumer is `scripts/assert-updater-archive-layout.mjs`, which runs on a shipped archive
// where no `otool` may be assumed. A parser bug there would either refuse a correct release or --
// worse -- accept a keeper whose slice requires a newer macOS than the app it ships beside, which is
// a keeper-less install the boot path only warns about. These cases pin both the encoding it reads
// and the refusals that keep an unreadable payload from being treated as "no floor, therefore fine".
import assert from "node:assert/strict";
import test from "node:test";

import {
  archForCpuType,
  compareVersions,
  decodeMachOVersion,
  KeeperMachOError,
  minimumVersionsByArch,
  readMachO,
} from "./lib/keeper-macho.mjs";

const CPU_ARM64 = 0x0100000c;
const CPU_X86_64 = 0x01000007;
const LC_BUILD_VERSION = 0x32;
const LC_VERSION_MIN_MACOSX = 0x24;

const encode = (text) => {
  const [major, minor, patch] = String(text).split(".").map((part) => Number.parseInt(part, 10) || 0);
  return ((major << 16) | (minor << 8) | patch) >>> 0;
};

/** One 64-bit little-endian Mach-O slice with an arbitrary set of load commands. */
function thin({ cputype, commands }) {
  const body = Buffer.concat(commands);
  const header = Buffer.alloc(32);
  header.writeUInt32LE(0xfeedfacf, 0);
  header.writeUInt32LE(cputype >>> 0, 4);
  header.writeUInt32LE(0, 8);
  header.writeUInt32LE(2, 12);
  header.writeUInt32LE(commands.length, 16);
  header.writeUInt32LE(body.length, 20);
  header.writeUInt32LE(0x00200085, 24);
  header.writeUInt32LE(0, 28);
  return Buffer.concat([header, body]);
}

function buildVersionCommand({ platform = 1, minos, sdk = "14.0" }) {
  const command = Buffer.alloc(24);
  command.writeUInt32LE(LC_BUILD_VERSION, 0);
  command.writeUInt32LE(24, 4);
  command.writeUInt32LE(platform, 8);
  command.writeUInt32LE(encode(minos), 12);
  command.writeUInt32LE(encode(sdk), 16);
  command.writeUInt32LE(0, 20);
  return command;
}

function versionMinCommand({ minos, sdk = "10.15" }) {
  const command = Buffer.alloc(16);
  command.writeUInt32LE(LC_VERSION_MIN_MACOSX, 0);
  command.writeUInt32LE(16, 4);
  command.writeUInt32LE(encode(minos), 8);
  command.writeUInt32LE(encode(sdk), 12);
  return command;
}

function fat(bodies) {
  const headerSize = 8 + 20 * bodies.length;
  const offsets = [];
  let cursor = Math.ceil(headerSize / 4096) * 4096;
  for (const body of bodies) {
    offsets.push(cursor);
    cursor = Math.ceil((cursor + body.bytes.length) / 4096) * 4096;
  }
  const out = Buffer.alloc(cursor);
  out.writeUInt32BE(0xcafebabe, 0);
  out.writeUInt32BE(bodies.length, 4);
  bodies.forEach((body, index) => {
    const at = 8 + 20 * index;
    out.writeUInt32BE(body.cputype, at);
    out.writeUInt32BE(0, at + 4);
    out.writeUInt32BE(offsets[index], at + 8);
    out.writeUInt32BE(body.bytes.length, at + 12);
    out.writeUInt32BE(12, at + 16);
    body.bytes.copy(out, offsets[index]);
  });
  return out;
}

function thinSlice({ cputype, minos, sdk, commands = null }) {
  const bytes = thin({ cputype, commands: commands ?? [buildVersionCommand({ minos, sdk })] });
  return { cputype, bytes };
}

test("reads the minimum OS of each slice of a universal binary", () => {
  const bytes = fat([
    thinSlice({ cputype: CPU_ARM64, minos: "11.0" }),
    thinSlice({ cputype: CPU_X86_64, minos: "10.15" }),
  ]);
  const macho = readMachO(bytes);
  assert.equal(macho.kind, "fat");
  assert.deepEqual(minimumVersionsByArch(macho), { arm64: "11.0.0", x86_64: "10.15.0" });
  assert.deepEqual(
    macho.slices.map((slice) => slice.sdk),
    ["14.0.0", "14.0.0"],
  );
  assert.deepEqual(
    macho.slices.map((slice) => slice.minosSource),
    ["LC_BUILD_VERSION", "LC_BUILD_VERSION"],
  );
});

test("reads a thin binary and the legacy LC_VERSION_MIN_MACOSX command", () => {
  const bytes = thin({ cputype: CPU_X86_64, commands: [versionMinCommand({ minos: "10.13" })] });
  const macho = readMachO(bytes);
  assert.equal(macho.kind, "thin");
  assert.equal(macho.slices.length, 1);
  assert.equal(macho.slices[0].arch, "x86_64");
  assert.equal(macho.slices[0].minos, "10.13.0");
  assert.equal(macho.slices[0].minosSource, "LC_VERSION_MIN_MACOSX");
});

test("a slice with no floor command is reported as having none, never as passing", () => {
  // Some object files carry no floor at all. The archive checker refuses such a payload explicitly;
  // this is the fact it depends on.
  const bytes = thin({ cputype: CPU_ARM64, commands: [] });
  const macho = readMachO(bytes);
  assert.equal(macho.slices[0].minos, null);
  assert.deepEqual(minimumVersionsByArch(macho), {});
});

test("refuses bytes that are not a Mach-O", () => {
  assert.throws(() => readMachO(Buffer.from("this is not a mach-o binary")), (error) => {
    assert.ok(error instanceof KeeperMachOError);
    assert.equal(error.code, "MACHO_MAGIC_UNSUPPORTED");
    return true;
  });
});

test("refuses a truncated universal header and a slice past the end of the file", () => {
  const truncatedHeader = Buffer.alloc(16);
  truncatedHeader.writeUInt32BE(0xcafebabe, 0);
  truncatedHeader.writeUInt32BE(4, 4);
  assert.throws(() => readMachO(truncatedHeader), /MACHO_TRUNCATED/);

  const bytes = fat([thinSlice({ cputype: CPU_ARM64, minos: "11.0" })]);
  // Claim a slice four times the file's length: hashing a short buffer is how a wrong size turns
  // into a wrong identity, so this must refuse rather than clamp.
  const corrupted = Buffer.from(bytes);
  corrupted.writeUInt32BE(corrupted.length + 4096, 8 + 12);
  assert.throws(() => readMachO(corrupted), /MACHO_TRUNCATED/);
});

test("refuses a load command whose size runs past the command area", () => {
  const command = buildVersionCommand({ minos: "11.0" });
  command.writeUInt32LE(4096, 4);
  const bytes = thin({ cputype: CPU_ARM64, commands: [command] });
  assert.throws(() => readMachO(bytes), /MACHO_TRUNCATED/);
});

test("version encoding and comparison match the toolchain's meaning", () => {
  assert.equal(decodeMachOVersion(encode("10.15")), "10.15.0");
  assert.equal(decodeMachOVersion(encode("11.0.1")), "11.0.1");
  assert.equal(compareVersions("11.0", "10.15"), 1);
  assert.equal(compareVersions("10.15", "10.15.0"), 0);
  assert.equal(compareVersions("10.9", "10.15"), -1);
  assert.equal(compareVersions("12.0", "11.9.9"), 1);
});

test("architecture names cover the two shipped slices", () => {
  assert.equal(archForCpuType(CPU_ARM64), "arm64");
  assert.equal(archForCpuType(CPU_X86_64), "x86_64");
  assert.equal(archForCpuType(12), "arm");
  assert.equal(archForCpuType(7), "i386");
  assert.equal(archForCpuType(99), "cpu_0x63");
});
