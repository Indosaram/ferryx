// Negative and positive coverage for the Phase 0 dSYM release gate (scripts/lib/release-dsym.mjs).
//
// No real build is required: `dwarfdump` is injected, so a fixture pair of a "binary" and a
// "dSYM" can carry whatever UUIDs the case needs. The point of the negative cases is that a dSYM
// built from a different link is REFUSED, not tolerated -- a mismatched dSYM symbolicate nothing
// and would send triage after the wrong source lines.
import test from "node:test";
import assert from "node:assert/strict";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  DsymGateError,
  assertDsymGate,
  findDsymBundles,
  normalizeUuid,
  parseDwarfdumpSlices,
  parseDwarfdumpStatistics,
  parseDwarfdumpUuids,
  produceDsymFromSlices,
  symbolCounts,
  strippedForShipping,
  uuidsOf,
} from "./lib/release-dsym.mjs";

const ARM64 = "883BC83E-B8FB-373E-9A6C-14C1FF47A2F6";
const X86_64 = "8426AD72-1580-30BD-B8A2-0A05B4672A9F";
const OTHER = "4A1B2C3D-0000-1111-2222-333344445555";

function withTmp(fn) {
  const root = mkdtempSync(join(tmpdir(), "ferryx-dsym-gate-"));
  try {
    return fn(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

/** A fake Mach-O the gate can stat, plus the dwarfdump answer that describes it. */
function fixture(root, { binaryUuids, dsymUuids = null, dsymName = "ferryx.dSYM", dsymFunctions = 12 }) {
  const binary = join(root, "Ferryx.app", "Contents", "MacOS", "ferryx");
  mkdirSync(join(binary, ".."), { recursive: true });
  writeFileSync(binary, "not a real Mach-O; the gate only reads dwarfdump's answer\n");
  const dsymDir = join(root, "dsyms");
  mkdirSync(dsymDir, { recursive: true });
  const bundle = join(dsymDir, dsymName);
  if (dsymUuids) {
    const dsym = join(bundle, "Contents", "Resources", "DWARF", "ferryx");
    mkdirSync(join(dsym, ".."), { recursive: true });
    writeFileSync(dsym, "");
  }
  const exec = (command, args) => {
    const target = args[args.length - 1];
    const toDwarfdump = (uuids) =>
      uuids.map((uuid) => `UUID: ${uuid} (arm64e) ${target}`).join("\n") + "\n";
    const statistics = (count) =>
      `${JSON.stringify(
        { version: 9, file: `${target}(arm64e)`, format: "Mach-O 64-bit", "#functions": count },
        null,
        2,
      )}\n`;
    if (command === "dwarfdump" && args[0] === "--statistics" && target.startsWith(bundle)) {
      return { status: 0, stdout: statistics(dsymFunctions), stderr: "" };
    }
    if (command === "dwarfdump" && args[0] === "--statistics") {
      return { status: 0, stdout: statistics(0), stderr: "" };
    }
    if (command === "dwarfdump" && target === binary) return { status: 0, stdout: toDwarfdump(binaryUuids), stderr: "" };
    if (command === "dwarfdump" && target.startsWith(dsymDir) && dsymUuids) {
      return { status: 0, stdout: toDwarfdump(dsymUuids), stderr: "" };
    }
    if (command === "dwarfdump") {
      return { status: 1, stdout: "", stderr: `error: ${target}: No such file or directory` };
    }
    return { status: 1, stdout: "", stderr: `unexpected command ${command}` };
  };
  return { binary, dsymDir, bundle, exec };
}

function refused(fn, code) {
  assert.throws(
    fn,
    (error) => error instanceof DsymGateError && error.code === code && error.message.startsWith(`${code}: `),
  );
}

test("a dSYM carrying the built binary's UUID passes the gate", () => withTmp((root) => {
  const { binary, dsymDir, bundle, exec } = fixture(root, { binaryUuids: [ARM64], dsymUuids: [ARM64] });
  const result = assertDsymGate({ binaryPath: binary, dsymDir, exec });
  assert.deepEqual(result.binaryUuids, [ARM64]);
  assert.deepEqual(result.bundles.map((entry) => entry.path), [bundle]);
}));

test("a MISMATCHED dSYM is refused, never warned past", () => withTmp((root) => {
  const { binary, dsymDir, exec } = fixture(root, { binaryUuids: [ARM64], dsymUuids: [OTHER] });
  refused(() => assertDsymGate({ binaryPath: binary, dsymDir, exec }), "DSYM_UUID_MISMATCH");
  // The refusal names both sides so the operator can see which build's dSYM sits in the archive.
  try {
    assertDsymGate({ binaryPath: binary, dsymDir, exec });
  } catch (error) {
    assert.match(error.message, new RegExp(ARM64));
    assert.match(error.message, new RegExp(OTHER));
    assert.deepEqual(error.detail.missing, [ARM64]);
  }
}));

test("an empty dSYM archive is refused with DSYM_MISSING", () => withTmp((root) => {
  const { binary, dsymDir, exec } = fixture(root, { binaryUuids: [ARM64], dsymUuids: null });
  refused(() => assertDsymGate({ binaryPath: binary, dsymDir, exec }), "DSYM_MISSING");
}));

test("a missing binary is refused before any dSYM is considered", () => withTmp((root) => {
  const { dsymDir, exec } = fixture(root, { binaryUuids: [ARM64], dsymUuids: [ARM64] });
  refused(() => assertDsymGate({ binaryPath: join(root, "absent"), dsymDir, exec }), "DSYM_BINARY_MISSING");
}));

test("a binary dwarfdump cannot describe is refused", () => withTmp((root) => {
  const { binary, dsymDir, exec } = fixture(root, { binaryUuids: [], dsymUuids: [ARM64] });
  refused(() => assertDsymGate({ binaryPath: binary, dsymDir, exec }), "DSYM_BINARY_UUID_UNREADABLE");
}));

test("every architecture of a universal binary must be covered", () => withTmp((root) => {
  const { binary, dsymDir, exec } = fixture(root, { binaryUuids: [ARM64, X86_64], dsymUuids: [ARM64] });
  // A crash in the x86_64 slice stays unsymbolicatable, so the release is still refused.
  refused(() => assertDsymGate({ binaryPath: binary, dsymDir, exec }), "DSYM_UUID_MISMATCH");
  try {
    assertDsymGate({ binaryPath: binary, dsymDir, exec });
  } catch (error) {
    assert.deepEqual(error.detail.missing, [X86_64]);
  }
}));

test("a matching dSYM is found among several unrelated bundles", () => withTmp((root) => {
  const { binary, dsymDir, bundle, exec } = fixture(root, { binaryUuids: [ARM64], dsymUuids: [ARM64] });
  const stray = join(dsymDir, "Ferryx Daemon.dSYM");
  mkdirSync(join(stray, "Contents", "Resources", "DWARF"), { recursive: true });
  const nestedExec = (command, args) => {
    const target = args[args.length - 1];
    if (command === "dwarfdump" && args[0] === "--statistics") {
      const count = target.startsWith(stray) ? 3 : 12;
      return {
        status: 0,
        stdout: `${JSON.stringify({ version: 9, file: `${target}(arm64e)`, "#functions": count })}\n`,
        stderr: "",
      };
    }
    if (command === "dwarfdump" && (target === binary || target.startsWith(dsymDir))) {
      const uuids = target.startsWith(stray) ? [OTHER] : [ARM64];
      return { status: 0, stdout: uuids.map((uuid) => `UUID: ${uuid} (arm64e) ${target}`).join("\n"), stderr: "" };
    }
    return exec(command, args);
  };
  const result = assertDsymGate({ binaryPath: binary, dsymDir, exec: nestedExec });
  assert.deepEqual(result.bundles.map((entry) => entry.path), [bundle]);
}));

test("bundle discovery finds nested dSYMs and stops at the bundle root", () => withTmp((root) => {
  const dsymDir = join(root, "dsyms");
  mkdirSync(join(dsymDir, "a.dSYM", "Contents", "Resources", "DWARF"), { recursive: true });
  mkdirSync(join(dsymDir, "nested", "b.dSYM"), { recursive: true });
  assert.deepEqual(findDsymBundles(dsymDir), [join(dsymDir, "a.dSYM"), join(dsymDir, "nested", "b.dSYM")]);
  assert.deepEqual(findDsymBundles(join(root, "absent")), []);
}));

test("produceDsymFromSlices names the debug-info cause when dsymutil extracts nothing", () => withTmp((root) => {
  const binary = join(root, "ferryx");
  writeFileSync(binary, "");
  const exec = () => ({ status: 1, stdout: "", stderr: "warning: no debug symbols in executable (-arch arm64)" });
  const outPath = join(root, "dsyms", "ferryx.dSYM");
  refused(() => produceDsymFromSlices({ slices: [binary], outPath, exec }), "DSYM_PRODUCE_FAILED");
  try {
    produceDsymFromSlices({ slices: [binary], outPath, exec });
  } catch (error) {
    assert.match(error.message, /CARGO_PROFILE_RELEASE_DEBUG=1/);
    assert.match(error.message, /CARGO_PROFILE_RELEASE_STRIP=none/);
  }
}));

test("stripping runs before signing and a failure is typed", () => withTmp((root) => {
  const binary = join(root, "ferryx");
  writeFileSync(binary, "");
  const calls = [];
  strippedForShipping(binary, (command, args) => {
    calls.push([command, ...args]);
    return { status: 0, stdout: "", stderr: "" };
  });
  assert.deepEqual(calls, [["strip", "-S", "-x", binary]]);
  refused(
    () => strippedForShipping(binary, () => ({ status: 1, stdout: "", stderr: "strip: bad object" })),
    "DSYM_STRIP_FAILED",
  );
}));

test("produceDsymFromSlices archives one bundle covering every architecture", () => withTmp((root) => {
  // The release form: the debug maps live in the per-architecture binaries rustc linked, so both
  // slices go to one dsymutil invocation and the bundle holds one DWARF file per arch.
  const arm = join(root, "ferryx-arm64");
  const intel = join(root, "ferryx-x86_64");
  writeFileSync(arm, "");
  writeFileSync(intel, "");
  const commands = [];
  const exec = (command, args) => {
    commands.push([command, ...args]);
    const outPath = args[args.indexOf("-o") + 1];
    if (command === "dsymutil") {
      mkdirSync(join(outPath, "Contents", "Resources", "DWARF"), { recursive: true });
      return { status: 0, stdout: "", stderr: "" };
    }
    if (args[0] === "--statistics") {
      const blocks = [ARM64, X86_64].map((_, index) =>
        JSON.stringify({ version: 9, file: `${outPath}(arch${index})`, "#functions": 3 }),
      );
      return { status: 0, stdout: blocks.join("\n"), stderr: "" };
    }
    const uuids = [ARM64, X86_64].map((uuid) => `UUID: ${uuid} (arch) ${args[1]}`);
    return { status: 0, stdout: uuids.join("\n"), stderr: "" };
  };
  const result = produceDsymFromSlices({ slices: [arm, intel], outPath: join(root, "dsyms", "ferryx.dSYM"), exec });
  assert.deepEqual(commands[0], ["dsymutil", arm, intel, "-o", join(root, "dsyms", "ferryx.dSYM")]);
  assert.deepEqual(result.uuids, [ARM64, X86_64]);
  assert.deepEqual(result.functions, { arch0: 3, arch1: 3 });
  refused(
    () => produceDsymFromSlices({ slices: [], outPath: join(root, "x.dSYM"), exec }),
    "DSYM_PRODUCE_FAILED",
  );
}));

test("uuid normalization and dwarfdump parsing cover real dwarfdump shapes", () => {
  assert.equal(normalizeUuid("883bc83e-b8fb-373e-9a6c-14c1ff47a2f6"), ARM64);
  assert.equal(normalizeUuid("883BC83EB8FB373E9A6C14C1FF47A2F6"), ARM64);
  assert.equal(normalizeUuid("883BC83E-B8FB-373E-9A6C"), null);
  assert.equal(normalizeUuid(""), null);
  // `dwarfdump --uuid /bin/ls` on this host prints one line per architecture, then exit status 0.
  const text = [
    "UUID: 8426AD72-1580-30BD-B8A2-0A05B4672A9F (x86_64) /bin/ls",
    "UUID: 883BC83E-B8FB-373E-9A6C-14C1FF47A2F6 (arm64e) /bin/ls",
    "warning: no debug symbols in executable",
  ].join("\n");
  assert.deepEqual(parseDwarfdumpUuids(text), [X86_64, ARM64]);
  assert.deepEqual(parseDwarfdumpUuids("error: /nope: No such file or directory"), []);
});

test("a dSYM that carries the UUID but describes no function is refused", () => withTmp((root) => {
  // Exactly what `strip="symbols"` produces, and what `dsymutil /bin/ls` reproduces: the UUID
  // matches, `dwarfdump --statistics` reports `#functions: 0`, and every crash frame stays
  // `ferryx +<offset>`. A UUID-only gate would pass this and ship an unusable archive.
  const { binary, dsymDir, exec } = fixture(root, { binaryUuids: [ARM64], dsymUuids: [ARM64], dsymFunctions: 0 });
  refused(() => assertDsymGate({ binaryPath: binary, dsymDir, exec }), "DSYM_EMPTY");
  try {
    assertDsymGate({ binaryPath: binary, dsymDir, exec });
  } catch (error) {
    assert.match(error.message, /CARGO_PROFILE_RELEASE_STRIP=none/);
    assert.equal(error.detail.empty.length, 1);
  }
}));

test("dwarfdump statistics parsing reads the per-architecture function count", () => {
  const text = [
    JSON.stringify({ version: 9, file: "/d/ls.dSYM/Contents/Resources/DWARF/ls(x86_64)", "#functions": 0 }),
    JSON.stringify({ version: 9, file: "/d/ls.dSYM/Contents/Resources/DWARF/ls(arm64e)", "#functions": 0 }),
  ].join("\n");
  assert.deepEqual(parseDwarfdumpStatistics(text), { x86_64: 0, arm64e: 0 });
  const real = JSON.stringify({ version: 9, file: "/d/hi2.dSYM/Contents/Resources/DWARF/hi2", "#functions": 2 });
  assert.deepEqual(parseDwarfdumpStatistics(real), { unknown: 2 });
  assert.deepEqual(parseDwarfdumpStatistics("error: no such file"), {});
});

test("uuid parsing keeps the architecture of each slice", () => {
  const text = [
    "UUID: 8426AD72-1580-30BD-B8A2-0A05B4672A9F (x86_64) /bin/ls",
    "UUID: 883BC83E-B8FB-373E-9A6C-14C1FF47A2F6 (arm64e) /bin/ls",
  ].join("\n");
  assert.deepEqual(parseDwarfdumpSlices(text), [
    { uuid: X86_64, arch: "x86_64" },
    { uuid: ARM64, arch: "arm64e" },
  ]);
});

test("the real dwarfdump agrees with the parser on a system binary", (t) => {
  // macOS only, and skip rather than fail where the toolchain is absent: the fixture cases above
  // carry the gate's contract, this case only pins the parser to the tool's actual output.
  if (process.platform !== "darwin") return t.skip("dwarfdump is macOS-only");
  const uuids = uuidsOf("/bin/ls");
  if (uuids.length === 0) return t.skip("dwarfdump not available");
  for (const uuid of uuids) assert.match(uuid, /^[0-9A-F]{8}(-[0-9A-F]{4}){3}-[0-9A-F]{12}$/);
});

test("a REAL dSYM pair with different UUIDs is refused by the real toolchain", (t) => {
  // The plan's negative fixture, built from two system binaries so no project build is needed:
  // dsymutil archives a dSYM for /bin/ls, and the gate is then asked about /bin/cat, whose UUID
  // differs. A real mismatched dSYM must be rejected, not merely fail to be found. The same
  // archive is also the real-world `DSYM_EMPTY` case, because system binaries ship stripped --
  // which is precisely why the gate checks functions and not UUIDs alone.
  if (process.platform !== "darwin") return t.skip("dsymutil and dwarfdump are macOS-only");
  if (spawnSync("which", ["dsymutil"]).status !== 0) return t.skip("dsymutil not available");
  const root = mkdtempSync(join(tmpdir(), "ferryx-dsym-real-"));
  try {
    const ls = join(root, "ls");
    const cat = join(root, "cat");
    copyFileSync("/bin/ls", ls);
    copyFileSync("/bin/cat", cat);
    const dsymDir = join(root, "dsyms");
    mkdirSync(dsymDir, { recursive: true });
    const made = spawnSync("dsymutil", [ls, "-o", join(dsymDir, "ls.dSYM")], { encoding: "utf8" });
    if (made.status !== 0) return t.skip(`dsymutil unavailable here: ${made.stderr.trim()}`);
    // Different binary, different UUID: refused before the emptiness check is even reached.
    refused(() => assertDsymGate({ binaryPath: cat, dsymDir }), "DSYM_UUID_MISMATCH");
    // Same binary, matching UUID -- and still refused, because a stripped system binary has no
    // DWARF to archive.
    refused(() => assertDsymGate({ binaryPath: ls, dsymDir }), "DSYM_EMPTY");
    // Host-independent: the slice set of /bin/ls varies by macOS release, so pin the invariant
    // (at least one slice, no functions in any of them) rather than the exact architectures.
    const counts = symbolCounts(join(dsymDir, "ls.dSYM"));
    assert.ok(counts, "dwarfdump must report statistics for the archived dSYM");
    assert.ok(Object.keys(counts).length >= 1, `expected at least one slice, got ${JSON.stringify(counts)}`);
    for (const [arch, functions] of Object.entries(counts)) {
      assert.equal(functions, 0, `${arch} must carry no functions`);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
