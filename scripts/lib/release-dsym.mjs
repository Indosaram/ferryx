// dSYM archive gate for macOS release artifacts (plan Phase 0).
//
// A daemon SIGSEGV leaves `ferryx +<offset>` frames in a `.ips` report; those become named
// functions only through the dSYM built from the same binary. The gate therefore compares the
// build UUIDs of the SHIPPED binary (`dwarfdump --uuid <binary>`) against the UUIDs of the
// archived dSYM and fails when any binary UUID has no dSYM carrying it — a mismatched dSYM is a
// gate failure, never a warning, because it symbolicate nothing and can only mislead triage.
import { existsSync, mkdirSync, readdirSync, statSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";

/** Typed refusal: `code` is the first blocking DSYM_* code and leads the message. */
export class DsymGateError extends Error {
  constructor(code, message, detail = {}) {
    super(`${code}: ${message}`);
    this.name = "DsymGateError";
    this.code = code;
    this.detail = detail;
  }
}

/** `883bc83e-b8fb-...` / `883BC83EB8FB...` -> dashed uppercase; anything else is not a UUID. */
export function normalizeUuid(raw) {
  const hex = String(raw ?? "").replace(/[\s-]/g, "");
  if (hex.length !== 32 || !/^[0-9a-fA-F]{32}$/.test(hex)) return null;
  const upper = hex.toUpperCase();
  return `${upper.slice(0, 8)}-${upper.slice(8, 12)}-${upper.slice(12, 16)}-${upper.slice(16, 20)}-${upper.slice(20)}`;
}

/** Parses `dwarfdump --uuid` output into `[{uuid, arch}]`, one per architecture, deduplicated. */
export function parseDwarfdumpSlices(text) {
  const slices = [];
  for (const line of String(text ?? "").split("\n")) {
    const match = /^\s*UUID:\s*(\S+)\s*(?:\(([^)]*)\))?/.exec(line);
    if (!match) continue;
    const uuid = normalizeUuid(match[1]);
    if (!uuid || slices.some((slice) => slice.uuid === uuid)) continue;
    slices.push({ uuid, arch: match[2] ?? null });
  }
  return slices;
}

/** Parses `dwarfdump --uuid` output into normalized UUIDs (one per architecture), deduplicated. */
export function parseDwarfdumpUuids(text) {
  return parseDwarfdumpSlices(text).map((slice) => slice.uuid);
}

/**
 * Parses `dwarfdump --statistics` into `{ "<arch>": functionCount }`.
 *
 * The tool prints one JSON object per architecture. `#functions` is the number of DWARF
 * subprograms with a location, which is exactly what symbolication needs: a dSYM can carry the
 * right UUID and still hold zero functions (a `strip="symbols"` binary produces one), and such a
 * dSYM names no frame in a crash report.
 */
export function parseDwarfdumpStatistics(text) {
  const counts = {};
  let depth = 0;
  let start = -1;
  const source = String(text ?? "");
  for (let index = 0; index < source.length; index += 1) {
    if (source[index] === "{" && depth++ === 0) start = index;
    if (source[index] === "}" && depth > 0 && --depth === 0) {
      const block = source.slice(start, index + 1);
      try {
        const parsed = JSON.parse(block);
        const functions = parsed["#functions"];
        if (typeof parsed.file === "string" && typeof functions === "number") {
          const arch = /\(([^)]+)\)\s*$/.exec(parsed.file);
          counts[arch ? arch[1] : "unknown"] = functions;
        }
      } catch {
        // Not a statistics block (interleaved warning text); nothing to record.
      }
    }
  }
  return counts;
}

function defaultExec(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8" });
  return { status: result.status ?? -1, stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
}

/** UUIDs of `path`, or `[]` when dwarfdump cannot read it. */
export function uuidsOf(path, exec) {
  return slicesOf(path, exec).map((slice) => slice.uuid);
}

/** `[{uuid, arch}]` of `path`, or `[]` when dwarfdump cannot read it. */
export function slicesOf(path, exec) {
  const run = exec ?? ((cmd, args) => defaultExec(cmd, args));
  const result = run("dwarfdump", ["--uuid", path]);
  if (result.status !== 0) return [];
  return parseDwarfdumpSlices(`${result.stdout}${result.stderr}`);
}

/**
 * `{ "<arch>": functionCount }` for `path`, or `null` when statistics cannot be read.
 *
 * Timing is what makes this affordable in a release gate: `dwarfdump --statistics` on a dSYM
 * returns in about ten milliseconds because it reads section headers, not the DWARF tree.
 */
export function symbolCounts(path, exec) {
  const run = exec ?? ((cmd, args) => defaultExec(cmd, args));
  const result = run("dwarfdump", ["--statistics", path]);
  if (result.status !== 0) return null;
  const counts = parseDwarfdumpStatistics(`${result.stdout}${result.stderr}`);
  return Object.keys(counts).length === 0 ? null : counts;
}

/** Every `*.dSYM` bundle under `dir`, depth-first, without following directory symlinks. */
export function findDsymBundles(dir) {
  if (!dir || !existsSync(dir)) return [];
  const found = [];
  const walk = (current, depth) => {
    if (depth > 4) return;
    if (current.endsWith(".dSYM")) {
      found.push(current);
      return;
    }
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      walk(join(current, entry.name), depth + 1);
    }
  };
  walk(dir, 0);
  return found.sort();
}

/**
 * Asserts a dSYM whose UUID matches the shipped binary exists in `dsymDir`.
 *
 * Both architecture UUIDs of a universal binary must be covered: symbolication of a crash in the
 * x86_64 slice fails without the x86_64 dSYM even when the arm64 one is present. A bundle that
 * matches by UUID but holds no function description is also refused (`DSYM_EMPTY`): that is what
 * `strip="symbols"` produces, and it names no frame in a crash report despite carrying the UUID.
 */
export function assertDsymGate({ binaryPath, dsymDir, exec = null }) {
  if (!binaryPath || !existsSync(binaryPath)) {
    throw new DsymGateError("DSYM_BINARY_MISSING", `built binary not found at ${binaryPath}`);
  }
  const run = exec ?? ((cmd, args) => defaultExec(cmd, args));
  const binarySlices = slicesOf(binaryPath, run);
  const binaryUuids = binarySlices.map((slice) => slice.uuid);
  if (binaryUuids.length === 0) {
    throw new DsymGateError(
      "DSYM_BINARY_UUID_UNREADABLE",
      `dwarfdump --uuid ${binaryPath} reported no UUID; cannot key a dSYM to this build`,
    );
  }
  const bundles = findDsymBundles(dsymDir);
  if (bundles.length === 0) {
    throw new DsymGateError(
      "DSYM_MISSING",
      `no dSYM under ${dsymDir}; the release must archive one for ${binaryUuids.join(", ")}`,
      { binaryUuids },
    );
  }
  const archived = [];
  for (const bundle of bundles) {
    const uuids = uuidsOf(bundle, run);
    if (uuids.length > 0) archived.push({ path: bundle, uuids });
  }
  const covered = new Set(archived.flatMap((entry) => entry.uuids));
  const missing = binaryUuids.filter((uuid) => !covered.has(uuid));
  if (missing.length > 0) {
    throw new DsymGateError(
      "DSYM_UUID_MISMATCH",
      `no archived dSYM carries ${missing.join(", ")} (binary UUIDs ${binaryUuids.join(", ")}; ` +
        `archived ${archived.map((entry) => `${entry.path} [${entry.uuids.join(", ")}]`).join("; ") || "none readable"})`,
      { binaryUuids, archived, missing },
    );
  }
  const matched = archived.filter((entry) => entry.uuids.some((uuid) => binaryUuids.includes(uuid)));
  // UUID correspondence is necessary, not sufficient: an archived bundle must also describe at least
  // one function, or it names no frame in a crash report.
  const empty = [];
  for (const entry of matched) {
    const counts = symbolCounts(entry.path, run);
    if (counts === null) continue;
    if (!Object.values(counts).some((count) => count > 0)) empty.push(entry.path);
  }
  if (empty.length > 0) {
    throw new DsymGateError(
      "DSYM_EMPTY",
      `${empty.join(", ")} carries the build UUID but describes no function; a crash frame would ` +
        `stay \`ferryx +<offset>\`. Produce it from a build with debug info ` +
        `(CARGO_PROFILE_RELEASE_DEBUG=1 and CARGO_PROFILE_RELEASE_STRIP=none) before stripping.`,
      { binaryUuids, empty },
    );
  }
  return { binaryPath, binarySlices, binaryUuids, bundles: matched };
}

/**
 * Builds one dSYM bundle from a binary's per-architecture slices and reports what it holds.
 *
 * A multi-architecture linker produces a combined binary, but the debug maps live in the per-architecture binaries
 * rustc linked, so `dsymutil` is given those and writes a single bundle holding one DWARF file per
 * slice -- exactly the bundle the UUID gate needs to cover every architecture. With one slice this
 * is the plain one-binary case. Verified against the real toolchain: one bundle, one function table
 * per arch, UUIDs matching the universal binary.
 */
export function produceDsymFromSlices({ slices, outPath, exec = null }) {
  if (!slices || slices.length === 0) {
    throw new DsymGateError("DSYM_PRODUCE_FAILED", "no binary slice to archive a dSYM from");
  }
  const run = exec ?? ((cmd, args) => defaultExec(cmd, args));
  const result = run("dsymutil", [...slices, "-o", outPath]);
  const combined = `${result.stdout}${result.stderr}`;
  if (result.status !== 0 || !existsSync(outPath)) {
    throw new DsymGateError(
      "DSYM_PRODUCE_FAILED",
      `dsymutil could not produce ${outPath}: ${combined.trim() || `exit ${result.status}`}. ` +
        `A binary linked with strip="symbols" or without debug info has nothing to extract; ` +
        `build with CARGO_PROFILE_RELEASE_DEBUG=1 and CARGO_PROFILE_RELEASE_STRIP=none, then strip -S.`,
    );
  }
  return { bundle: outPath, uuids: uuidsOf(outPath, run), functions: symbolCounts(outPath, run) };
}

/**
 * Strips a Mach-O binary for shipping after its dSYM was archived. Must run before codesign and
 * notarization, which seal the exact bytes that ship.
 */
export function strippedForShipping(binaryPath, exec = null) {
  const run = exec ?? ((cmd, args) => defaultExec(cmd, args));
  const result = run("strip", ["-S", "-x", binaryPath]);
  if (result.status !== 0) {
    const detail = `${result.stdout}${result.stderr}`.trim() || `exit ${result.status}`;
    throw new DsymGateError("DSYM_STRIP_FAILED", `strip failed for ${binaryPath}: ${detail}`);
  }
  return binaryPath;
}

/** True when `path` is a regular file (used to skip architectures a host did not build). */
export function isFile(path) {
  return existsSync(path) && statSync(path).isFile();
}
