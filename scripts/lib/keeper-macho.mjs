// The minimum-OS floor a bundled Mach-O declares, read from the file's own load commands.
//
// Why this exists at packaging time: `tauri.conf.json` promises `minimumSystemVersion` (10.15) and
// the updater archive ships a keeper that the daemon execs on the user's machine. If the keeper's
// arm64 or x86_64 slice declares a newer OS than the app does, the app launches and the keeper does
// not, and `run_keeper_install_pass` downgrades that failure to `tracing::warn!` -- so the shipped
// app would run keeper-less while looking healthy. Nothing else in the pipeline reads this field, so
// a keeper built on a newer SDK could silently raise the effective floor of a release.
//
// The comparison is made per architecture against the SAME archive's daemon binary rather than
// against `LSMinimumSystemVersion`: arm64 slices legitimately declare 11.0 while the app still
// supports 10.15 through its x86_64 slice, so a flat comparison against the plist would refuse
// every correct universal build. Per-arch bytes answer the question that matters -- "can this
// machine run the daemon it also has to run the keeper beside?" -- without that false positive.
//
// Parsed by hand (no `otool`/`vtool`) because the release gate must run on the archive alone, with
// no Xcode toolchain assumptions, and deterministically on Linux CI as well.

/** Thin Mach-O magic numbers, as they appear in the file's first four bytes. */
export const MH_MAGIC = 0xfeedface;
export const MH_MAGIC_64 = 0xfeedfacf;
/** Universal ("fat") container magics. */
export const FAT_MAGIC = 0xcafebabe;
export const FAT_MAGIC_64 = 0xcafebabf;

export const LC_VERSION_MIN_MACOSX = 0x24;
export const LC_BUILD_VERSION = 0x32;
/** `LC_BUILD_VERSION.platform` for macOS (`PLATFORM_MACOS` in `<mach-o/loader.h>`). */
export const PLATFORM_MACOS = 1;

const CPU_ARCH_ABI64 = 0x01000000;
const CPU_TYPE_X86 = 7;
const CPU_TYPE_ARM = 12;

/** Typed refusal: `code` leads the message so a caller can report what is unreadable. */
export class KeeperMachOError extends Error {
  constructor(code, message) {
    super(`${code}: ${message}`);
    this.name = "KeeperMachOError";
    this.code = code;
  }
}

/**
 * Architecture name for a `cputype` value: `arm64`, `x86_64`, `arm`, `i386`, else `cpu_<hex>`.
 *
 * Only the two slices a Ferryx release ships have to be recognised; naming the others instead of
 * throwing keeps a diagnostic useful when someone inspects an unexpected binary.
 */
export function archForCpuType(cputype) {
  if (cputype === (CPU_ARCH_ABI64 | CPU_TYPE_ARM)) return "arm64";
  if (cputype === (CPU_ARCH_ABI64 | CPU_TYPE_X86)) return "x86_64";
  if (cputype === CPU_TYPE_ARM) return "arm";
  if (cputype === CPU_TYPE_X86) return "i386";
  return `cpu_0x${(cputype >>> 0).toString(16)}`;
}

/** Unpacks the `xxxx.yy.zz` version encoding used by `minos`/`sdk` into `"major.minor.patch"`. */
export function decodeMachOVersion(value) {
  const major = (value >>> 16) & 0xffff;
  const minor = (value >>> 8) & 0xff;
  const patch = value & 0xff;
  return `${major}.${minor}.${patch}`;
}

/** Numeric comparison of dotted versions, missing components counting as zero. */
export function compareVersions(left, right) {
  const split = (text) =>
    String(text ?? "")
      .split(/[-+]/)[0]
      .split(".")
      .map((part) => {
        const value = Number.parseInt(part, 10);
        return Number.isNaN(value) ? 0 : value;
      });
  const a = split(left);
  const b = split(right);
  const width = Math.max(a.length, b.length, 3);
  for (let index = 0; index < width; index += 1) {
    const first = a[index] ?? 0;
    const second = b[index] ?? 0;
    if (first !== second) return first < second ? -1 : 1;
  }
  return 0;
}

function readSlice(buffer, base, limit) {
  if (base + 8 > limit) throw new KeeperMachOError("MACHO_TRUNCATED", "slice header runs past the file");
  const magicBE = buffer.readUInt32BE(base);
  let littleEndian;
  if (magicBE === MH_MAGIC || magicBE === MH_MAGIC_64) littleEndian = false;
  else {
    const magicLE = buffer.readUInt32LE(base);
    if (magicLE === MH_MAGIC || magicLE === MH_MAGIC_64) littleEndian = true;
    else throw new KeeperMachOError("MACHO_MAGIC_UNSUPPORTED", `no Mach-O magic at offset ${base}`);
  }
  const u32 = (offset) => (littleEndian ? buffer.readUInt32LE(offset) : buffer.readUInt32BE(offset));
  const is64 = u32(base) === MH_MAGIC_64;
  const headerSize = is64 ? 32 : 28;
  if (base + headerSize > limit) throw new KeeperMachOError("MACHO_TRUNCATED", "Mach-O header runs past the file");

  const cputype = u32(base + 4);
  const ncmds = u32(base + 16);
  const sizeofcmds = u32(base + 20);
  const commandsEnd = base + headerSize + sizeofcmds;
  if (commandsEnd > limit) {
    throw new KeeperMachOError("MACHO_TRUNCATED", "load commands run past the end of the slice");
  }

  let platform = null;
  let minos = null;
  let sdk = null;
  let source = null;
  let offset = base + headerSize;
  for (let index = 0; index < ncmds; index += 1) {
    if (offset + 8 > commandsEnd) throw new KeeperMachOError("MACHO_TRUNCATED", "a load command header is truncated");
    const command = u32(offset);
    const size = u32(offset + 4);
    if (size < 8 || offset + size > commandsEnd) {
      throw new KeeperMachOError("MACHO_TRUNCATED", `load command ${index} declares an impossible size ${size}`);
    }
    if (command === LC_BUILD_VERSION && size >= 24) {
      platform = u32(offset + 8);
      minos = decodeMachOVersion(u32(offset + 12));
      sdk = decodeMachOVersion(u32(offset + 16));
      source = "LC_BUILD_VERSION";
    } else if (command === LC_VERSION_MIN_MACOSX && size >= 16) {
      // Pre-Xcode-10 output: this command only ever describes macOS.
      platform = PLATFORM_MACOS;
      minos = decodeMachOVersion(u32(offset + 8));
      sdk = decodeMachOVersion(u32(offset + 12));
      source = "LC_VERSION_MIN_MACOSX";
    }
    offset += size;
  }

  return { arch: archForCpuType(cputype), cputype, platform, minos, sdk, minosSource: source };
}

/**
 * Reads a Mach-O (thin or universal) and returns one entry per architecture.
 *
 * Returns `{ kind, slices }` where `kind` is `"thin"` or `"fat"`. Each slice carries `arch`,
 * `platform`, `minos`, `sdk` and `minosSource`; `minos` is `null` when the file declares no floor
 * at all (some object files), which the caller must treat as unverifiable rather than as passing.
 */
export function readMachO(bytes) {
  const buffer = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes ?? []);
  if (buffer.length < 8) throw new KeeperMachOError("MACHO_TRUNCATED", "file is shorter than a Mach-O header");

  const magicBE = buffer.readUInt32BE(0);
  if (magicBE === FAT_MAGIC || magicBE === FAT_MAGIC_64) {
    const fat64 = magicBE === FAT_MAGIC_64;
    const count = buffer.readUInt32BE(4);
    if (count === 0 || count > 16) {
      throw new KeeperMachOError("MACHO_TRUNCATED", `universal header declares ${count} architectures`);
    }
    const entrySize = fat64 ? 32 : 20;
    if (8 + entrySize * count > buffer.length) {
      throw new KeeperMachOError("MACHO_TRUNCATED", "universal header runs past the file");
    }
    const slices = [];
    for (let index = 0; index < count; index += 1) {
      const at = 8 + entrySize * index;
      const offset = fat64 ? Number(buffer.readBigUInt64BE(at + 8)) : buffer.readUInt32BE(at + 8);
      const size = fat64 ? Number(buffer.readBigUInt64BE(at + 16)) : buffer.readUInt32BE(at + 12);
      if (offset + size > buffer.length) {
        throw new KeeperMachOError("MACHO_TRUNCATED", `slice ${index} runs past the end of the file`);
      }
      slices.push(readSlice(buffer, offset, offset + size));
    }
    return { kind: "fat", slices };
  }

  return { kind: "thin", slices: [readSlice(buffer, 0, buffer.length)] };
}

/** `{ arm64: "11.0", x86_64: "10.15" }` for a Mach-O, skipping slices that declare no floor. */
export function minimumVersionsByArch(macho) {
  const floors = {};
  for (const slice of macho?.slices ?? []) {
    if (typeof slice.minos === "string") floors[slice.arch] = slice.minos;
  }
  return floors;
}
