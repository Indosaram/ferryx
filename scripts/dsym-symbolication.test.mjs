// Phase 0 acceptance rules: the forced-crash symbolication verdict (scripts/lib/dsym-symbolication.mjs)
// and the harness plumbing that decides it (scripts/qa/dsym-symbolication-acceptance.mjs).
//
// Nothing here builds, crashes a process, or shells out to Apple's tools: `spawn`, `watch` and the
// command runner are injected, so the DECISION is pinned without a Darwin host. The real toolchain
// is exercised by the harness itself, on a host that has one.
import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, utimesSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, basename, join, resolve } from "node:path";
import { gunzipSync, gzipSync } from "node:zlib";

import {
  ANONYMOUS,
  APP_BINARY_MEMBER,
  NAMED,
  binaryFrames,
  buildBinaryIdentity,
  buildForcedCrashRecord,
  classifyBody,
  crashImageUuids,
  crashReportIdentity,
  evaluateBinaryIdentity,
  evaluateMismatchedDsym,
  evaluateForcedCrashProvenance,
  evaluatePackagedBinaryIdentity,
  evaluateSymbolication,
  machOUuids,
  namesBinary,
  parseTarEntries,
  parseSymbolicatedFrames,
  readTarGzMember,
  renderSymbolicationSummary,
  resolveSymbolicationBundles,
  sha256Hex,
} from "./lib/dsym-symbolication.mjs";
import {
  BINARY_FRAME_LABEL,
  CRASH_SYMBOLICATOR_TOOL,
  DESYMBOLICATED_REPORT_NAME,
  EnvironmentGap,
  HarnessError,
  REDACTED_HOME_PREFIX,
  acquireForcedCrash,
  activeDeveloperDir,
  assertReportSource,
  assertSafeTarget,
  awaitNewCrashReport,
  collectBundle,
  desymbolicateReport,
  findSymbolicatecrash,
  forceSegv,
  inspectPackagedArtifact,
  isJsonCrashReport,
  isolatedEnv,
  listCrashReports,
  matchesRedactedProcPath,
  parseArgs,
  produceMismatchedDsym,
  reconcileReportProcPaths,
  renderJsonCrashFrames,
  reportNamesUuid,
  resolveCrashSymbolicator,
  resolveSymbolicatecrash,
  runSymbolicationChecks,
  symbolicate,
  symbolicationEnv,
} from "./qa/dsym-symbolication-acceptance.mjs";
// The release gate another owner maintains. It is only READ here, to prove the report this harness
// writes satisfies the gate as it actually ships, rather than a shape this suite invented.
import {
  assertSymbolicationGate,
  buildDsymArchiveManifest,
  evaluateSymbolicationEvidence,
  writeDsymArchiveManifest,
} from "./lib/release-symbolication.mjs";

const ARM64 = "883BC83E-B8FB-373E-9A6C-14C1FF47A2F6";
/** A second, distinct build UUID (the x86_64 slice of a universal binary). */
const X86 = "4A1B2C3D-0000-1111-2222-333344445555";

/**
 * The crash report's own header time: `04:00` in a `+0900` zone, which is 19:00Z the day before.
 *
 * The offset is the trap these fixtures exist to survive: a provenance fixture that wrote
 * `04:00Z` would look "after the spawn" while the header actually names an instant ten hours
 * earlier, and the ordering check would be asserted against the wrong clock.
 */
const REPORT_TIMESTAMP = "2026-11-16 04:00:00.00 +0900";
/** {@link REPORT_TIMESTAMP} as ISO, i.e. the instant the header names, printed in UTC. */
const REPORT_WRITTEN_AT = "2026-11-15T19:00:00.000Z";
/** A spawn instant before {@link REPORT_WRITTEN_AT}, so the report provably followed the signal. */
const SPAWNED_AT = "2026-11-15T18:59:00.000Z";
/** The digest recorded before the signal; a fixture value, since no process here really runs. */
const PRE_CRASH_SHA = sha256Hex(Buffer.from("ferryx-pre-crash-bytes"));

/**
 * An `.ips` header timestamp in macOS's own `YYYY-MM-DD HH:MM:SS.ss +ZZZZ` form, for `atMs`.
 *
 * Used where the fixture cannot know the host clock in advance but the ordering clause still has to
 * hold: the caller passes an instant AFTER the spawn it is describing. `+0000` is a real offset, so
 * this exercises the offset normalization instead of sidestepping it with a UTC string macOS would
 * never write.
 */
function ipsTimestamp(atMs) {
  const iso = new Date(atMs).toISOString();
  return `${iso.slice(0, 10)} ${iso.slice(11, 19)}.00 +0000`;
}

/**
 * A fixed "now" for the `acquireForcedCrash` fixtures.
 *
 * Those tests inject `now`, so the spawn instant is a constant instead of the host clock. Without
 * it the report the fixture writes must be dated after an instant the fixture cannot know when it
 * is written, and the ordering clause would be asserted against whatever second the run happened to
 * start in.
 */
const CLOCK_MS = Date.parse("2026-11-16T04:00:00.000Z");
/** A crash report timestamp the fixture writes a little after {@link CLOCK_MS}. */
const REPORT_AFTER_CLOCK = ipsTimestamp(CLOCK_MS + 5_000);

function withTmp(fn) {
  const root = mkdtempSync(join(tmpdir(), "ferryx-dsym-accept-"));
  const cleanup = () => rmSync(root, { recursive: true, force: true });
  const result = fn(root);
  if (result && typeof result.then === "function") return result.finally(cleanup);
  cleanup();
  return result;
}

/** A modern .ips: one-line header, then the JSON body carrying usedImages. */
const MODERN_REPORT = [
  '{"app_name":"ferryx","bug_type":"309","timestamp":"2026-11-15 08:50:37.00 +0900"}',
  JSON.stringify({
    usedImages: [
      { name: "libsystem_kernel.dylib", uuid: "11111111-1111-1111-1111-111111111111" },
      { name: "ferryx", uuid: "883bc83e-b8fb-373e-9a6c-14c1ff47a2f6" },
    ],
  }),
].join("\n");

/**
 * The same shape, but stating the pid, executable and header time a crash report is bound to.
 *
 * `timestamp` is the report's own header time, which the provenance check orders against the spawn
 * and which `buildForcedCrashRecord` normalizes into `reportWrittenAt`. It is a parameter because
 * the header is local time with an offset: a fixture that wants "written after the spawn" has to
 * say whose spawn it means.
 */
function reportForPid({ pid = 4242, procPath = "/tmp/ferryx", uuids = [ARM64], timestamp = REPORT_TIMESTAMP } = {}) {
  return [
    `{"app_name":"ferryx","bug_type":"309","timestamp":"${timestamp}"}`,
    JSON.stringify({
      pid,
      procPath,
      usedImages: [
        { name: "libsystem_kernel.dylib", uuid: "11111111-1111-1111-1111-111111111111" },
        ...uuids.map((uuid) => ({ name: "ferryx", uuid })),
      ],
    }),
  ].join("\n");
}

/** A minimal thin Mach-O carrying only `LC_UUID`: enough to prove the UUID walk, nothing more. */
function macho({ uuid = ARM64, cputype = 0x0100000c } = {}) {
  const header = Buffer.alloc(32);
  header.writeUInt32LE(0xfeedfacf, 0);
  header.writeUInt32LE(cputype, 4);
  header.writeUInt32LE(1, 16); // ncmds
  header.writeUInt32LE(24, 20); // sizeofcmds
  const command = Buffer.alloc(24);
  command.writeUInt32LE(0x1b, 0); // LC_UUID
  command.writeUInt32LE(24, 4);
  Buffer.from(uuid.replace(/-/g, ""), "hex").copy(command, 8);
  return Buffer.concat([header, command]);
}

/** A universal container holding the given thin slices, at 4 KiB-aligned offsets. */
function fatMacho(slices) {
  const header = Buffer.alloc(8 + slices.length * 20);
  header.writeUInt32BE(0xcafebabe, 0);
  header.writeUInt32BE(slices.length, 4);
  const aligned = [];
  let cursor = Math.ceil(header.length / 4096) * 4096;
  slices.forEach((slice, index) => {
    const entry = 8 + index * 20;
    header.writeUInt32BE(0x0100000c, entry);
    header.writeUInt32BE(0, entry + 4);
    header.writeUInt32BE(cursor, entry + 8);
    header.writeUInt32BE(slice.length, entry + 12);
    header.writeUInt32BE(12, entry + 16);
    aligned.push({ offset: cursor, bytes: slice });
    cursor = Math.ceil((cursor + slice.length) / 4096) * 4096;
  });
  const total = cursor;
  const out = Buffer.alloc(total);
  header.copy(out, 0);
  for (const { offset, bytes } of aligned) bytes.copy(out, offset);
  return out;
}

/** One ustar entry; only the fields `parseTarEntries` reads are populated, but it is a real header. */
function tarEntry(name, bytes) {
  const header = Buffer.alloc(512);
  header.write(name, 0, 100, "utf8");
  header.write("0000644", 100, 8, "utf8"); // mode
  header.write("0000000", 108, 8, "utf8"); // uid
  header.write("0000000", 116, 8, "utf8"); // gid
  header.write(bytes.length.toString(8).padStart(11, "0"), 124, 12, "utf8");
  header.write("00000000000", 136, 12, "utf8"); // mtime
  header.write("        ", 148, 8, "utf8"); // checksum placeholder
  header.write("0", 156, 1, "utf8"); // regular file
  header.write("ustar\0", 257, 6, "utf8");
  header.write("00", 263, 2, "utf8");
  let sum = 0;
  for (const byte of header) sum += byte;
  header.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148, 8, "utf8");
  const padded = Buffer.alloc(Math.ceil(bytes.length / 512) * 512);
  bytes.copy(padded, 0);
  return Buffer.concat([header, padded]);
}

/** A real `.tar.gz` holding `members` as `{name, bytes}`. */
function tarGz(members) {
  return gzipSync(Buffer.concat([...members.map((m) => tarEntry(m.name, m.bytes)), Buffer.alloc(1024)]));
}

/** Real symbolicatecrash column shape: anonymous, then a named frame. */
const ANONYMOUS_REPORT = [
  "Thread 0 Crashed:: tokio-runtime-worker  DispatchQueue_1",
  "0   ferryx                 0x0000000104a3f2a0 0x104000000 + 7401632",
  "1   libsystem_kernel.dylib 0x000000018f0d2b7c 0x18f000000 + 863100",
].join("\n");

const NAMED_REPORT = [
  "Thread 0 Crashed:: tokio-runtime-worker  DispatchQueue_1",
  "0   ferryx                 0x0000000104a3f2a0 ferryx::terminal::pty::PtySession::read_frame + 32 (pty.rs:412)",
  "1   ferryx                 0x0000000104a3f2c0 ferryx::daemon::server::DaemonServer::serve + 8 (server.rs:97)",
  "2   libsystem_kernel.dylib 0x000000018f0d2b7c kevent + 12",
].join("\n");

const QUESTION_MARKS = [
  "Thread 0 Crashed:: tokio-runtime-worker",
  "0   ferryx                 0x0000000104a3f2a0 ???",
].join("\n");

test("crash UUIDs are read from a modern .ips and normalized", () => {
  assert.deepEqual(crashImageUuids(MODERN_REPORT), [ARM64]);
  // Undashed input normalizes to the same canonical form.
  assert.deepEqual(crashImageUuids(`{"usedImages":[{"name":"ferryx","uuid":"883bc83eb8fb373e9a6c14c1ff47a2f6"}]}`), [ARM64]);
});

test("crash UUIDs fall back to a legacy Binary Images section", () => {
  const legacy = [
    '{"app_name":"ferryx","bug_type":"309"}',
    "Binary Images:",
    "       0x100000000 - 0x1009ffff +ferryx (2026.1113.2) <4a1b2c3d-0000-1111-2222-333344445555> /Applications/Ferryx.app/Contents/MacOS/ferryx",
    "       0x180000000 - 0x180ffffff  libsystem_kernel.dylib ( ) <99999999-0000-1111-2222-333344445555> /usr/lib/system/libsystem_kernel.dylib",
  ].join("\n");
  assert.deepEqual(crashImageUuids(legacy), ["4A1B2C3D-0000-1111-2222-333344445555"]);
  assert.deepEqual(crashImageUuids("not a crash report"), []);
});

test("namesBinary matches the binary and not a path fragment", () => {
  assert.equal(namesBinary(MODERN_REPORT), true);
  assert.equal(namesBinary('{"usedImages":[{"name":"libsystem_kernel.dylib"}]}'), false);
  // `ferryx` inside `my-ferryx-helper` is a different image, not the binary under acceptance.
  assert.equal(namesBinary("2   my-ferryx-helper 0x1 ???"), false);
});

test("frames are parsed and classified: offset-only is anonymous, a symbol is named", () => {
  const anonymous = parseSymbolicatedFrames(ANONYMOUS_REPORT);
  assert.equal(anonymous.length, 2);
  assert.equal(anonymous[0].image, "ferryx");
  assert.equal(anonymous[0].kind, ANONYMOUS);
  const named = parseSymbolicatedFrames(NAMED_REPORT);
  assert.equal(named.filter((frame) => frame.kind === NAMED).length, 3);
  assert.match(named[0].body, /PtySession::read_frame/);
  // But only two of them belong to the binary under acceptance; the third is libsystem.
  assert.equal(binaryFrames(named).filter((frame) => frame.kind === NAMED).length, 2);
});

test("??? counts as anonymous: a gate that passed it would pass the state Phase 0 ends", () => {
  const [frame] = parseSymbolicatedFrames(QUESTION_MARKS);
  assert.equal(frame.kind, ANONYMOUS);
  assert.equal(classifyBody("???"), ANONYMOUS);
  assert.equal(classifyBody("0x104000000 + 7401632"), ANONYMOUS);
  assert.equal(classifyBody("+ 7401632"), ANONYMOUS);
  assert.equal(classifyBody(""), ANONYMOUS);
  assert.equal(classifyBody("main + 32 (main.rs:10)"), NAMED);
  assert.equal(classifyBody("foo::bar() + 8"), NAMED);
});

test("only frames of the binary under acceptance are considered", () => {
  const frames = parseSymbolicatedFrames(NAMED_REPORT);
  assert.deepEqual(binaryFrames(frames).map((frame) => frame.index), [0, 1]);
  // A system library that happens to be named proves nothing about the archived dSYM.
  assert.deepEqual(binaryFrames(frames, ["libsystem_kernel"]).map((frame) => frame.index), [2]);
});

test("acceptance requires a named frame of the crashed binary", () => {
  const good = evaluateSymbolication({ crashText: MODERN_REPORT, symbolicatedText: NAMED_REPORT });
  assert.equal(good.ok, true);
  assert.equal(good.namedFrames.length, 2);
  assert.equal(good.checks.namedFramePresent.ok, true);

  // The exact pre-Phase-0 state: right binary, every frame still `ferryx +<offset>`.
  const bad = evaluateSymbolication({ crashText: MODERN_REPORT, symbolicatedText: ANONYMOUS_REPORT });
  assert.equal(bad.ok, false);
  assert.equal(bad.checks.namedFramePresent.ok, false);
  assert.equal(bad.anonymousFrames.length, 1);
});

test("a named SYSTEM frame does not satisfy the acceptance", () => {
  // Only libsystem resolved; the ferryx frame stayed an offset.
  const text = [
    "0   ferryx                 0x0000000104a3f2a0 0x104000000 + 7401632",
    "1   libsystem_kernel.dylib 0x000000018f0d2b7c kevent + 12",
  ].join("\n");
  const verdict = evaluateSymbolication({ crashText: MODERN_REPORT, symbolicatedText: text });
  assert.equal(verdict.ok, false);
  assert.deepEqual(verdict.namedFrames, []);
});

test("a report about another binary cannot prove the archive", () => {
  const safari = '{"usedImages":[{"name":"Safari","uuid":"11111111-1111-1111-1111-111111111111"}]}';
  const verdict = evaluateSymbolication({ crashText: safari, symbolicatedText: NAMED_REPORT });
  assert.equal(verdict.ok, false);
  assert.equal(verdict.checks.crashReportNamesBinary.ok, false);
});

test("the negative run needs BOTH a typed refusal and zero named frames", () => {
  const good = evaluateMismatchedDsym({ gateCode: "DSYM_UUID_MISMATCH", symbolicatedText: ANONYMOUS_REPORT });
  assert.equal(good.ok, true);
  assert.equal(good.checks.gateRefusedMismatch.ok, true);

  // A gate that merely failed to find a bundle is not the mismatch proof the plan asks for.
  const wrongCode = evaluateMismatchedDsym({ gateCode: "DSYM_MISSING", symbolicatedText: ANONYMOUS_REPORT });
  assert.equal(wrongCode.ok, false);

  // A mismatched dSYM that somehow named a frame would mean the gate is not doing its job.
  const named = evaluateMismatchedDsym({ gateCode: "DSYM_UUID_MISMATCH", symbolicatedText: NAMED_REPORT });
  assert.equal(named.ok, false);
  assert.equal(named.checks.mismatchNamesNothing.ok, false);
});

test("the summary states the verdict and every check", () => {
  const positive = evaluateSymbolication({ crashText: MODERN_REPORT, symbolicatedText: NAMED_REPORT });
  const negative = evaluateMismatchedDsym({ gateCode: "DSYM_UUID_MISMATCH", symbolicatedText: ANONYMOUS_REPORT });
  const text = renderSymbolicationSummary({
    verdict: "PASS",
    binary: { path: "/tmp/ferryx", uuids: [ARM64] },
    dsymArchive: { dir: "/tmp/dsyms", uuids: [ARM64] },
    dsymTargets: { paths: ["/tmp/dsyms/ferryx.dSYM"] },
    crash: { path: "/tmp/a.ips", uuids: [ARM64] },
    forcedCrash: {
      way: "forced-segv",
      pid: 4242,
      signal: "SIGSEGV",
      spawnedBinary: "/tmp/ferryx",
      reportPids: [4242],
    },
    packagedBinary: { artifact: { path: "/tmp/Ferryx.app.tar.gz", memberPath: APP_BINARY_MEMBER }, sha256: "abc123" },
    symbolicated: { namedFrames: 2 },
    negative: { gateCode: "DSYM_UUID_MISMATCH", namedFrames: 0 },
    checks: { ...positive.checks, ...negative.checks },
    reportDir: "/tmp/report",
  });
  assert.match(text, /^verdict: PASS/);
  assert.match(text, /PASS namedFramePresent/);
  assert.match(text, new RegExp(ARM64));
  // The binding an operator needs is printed too, not just stored in the JSON.
  assert.match(text, /provenance: forced-segv \(pid 4242, SIGSEGV, \/tmp\/ferryx\)/);
  assert.match(text, /crash report pid\(s\): 4242/);
  assert.match(text, new RegExp(`-> ${APP_BINARY_MEMBER}`));
  assert.match(text, /dSYM bundles symbolicated: \/tmp\/dsyms\/ferryx\.dSYM/);
});

test("argv parsing rejects unknown flags and missing values", () => {
  const args = parseArgs(["--binary", "/tmp/ferryx", "--dsym", "/tmp/dsyms", "--artifact", "/tmp/a.tar.gz", "--json"]);
  assert.deepEqual(args, {
    binary: "/tmp/ferryx",
    dsym: "/tmp/dsyms",
    artifact: "/tmp/a.tar.gz",
    out: null,
    ips: null,
    json: true,
    dryRun: false,
  });
  assert.throws(() => parseArgs(["--binary", "/tmp/ferryx"]), (error) => error.code === "USAGE");
  assert.throws(() => parseArgs(["--dsym", "/tmp/d"]), (error) => error.code === "USAGE");
  // Without the shipped tarball the symbolicated bytes are not bound to anything published.
  assert.throws(() => parseArgs(["--binary", "/tmp/ferryx", "--dsym", "/tmp/d"]), (error) => error.code === "USAGE");
  assert.throws(() => parseArgs(["--binary", "--dsym"]), (error) => error.code === "USAGE");
  assert.throws(() => parseArgs(["--bogus"]), (error) => error.code === "USAGE");
});

test("Mach-O UUIDs are read from thin and universal images", () => {
  assert.deepEqual(machOUuids(macho()), [ARM64]);
  // A universal binary carries one LC_UUID per slice; both must be found or the x86_64 dSYM
  // requirement would look satisfied by the arm64 UUID alone.
  assert.deepEqual(machOUuids(fatMacho([macho(), macho({ uuid: X86, cputype: 0x01000007 })])), [ARM64, X86]);
  // Not a Mach-O: no UUID, rather than a guessed one.
  assert.deepEqual(machOUuids(Buffer.from("#!/bin/sh\necho hi\n")), []);
  assert.deepEqual(machOUuids(Buffer.alloc(0)), []);
});

test("a tarball member is read by exact path, never by suffix", () => {
  const binary = macho();
  const archive = tarGz([
    { name: `./${APP_BINARY_MEMBER}`, bytes: binary },
    { name: "./Ferryx.app/Contents/MacOS/helper", bytes: Buffer.from("other") },
    { name: "./Ferryx.app/Contents/Info.plist", bytes: Buffer.from("<plist/>") },
  ]);
  assert.deepEqual(parseTarEntries(gunzipSync(archive)).map((entry) => entry.name), [
    `./${APP_BINARY_MEMBER}`,
    "./Ferryx.app/Contents/MacOS/helper",
    "./Ferryx.app/Contents/Info.plist",
  ]);
  const member = readTarGzMember({ bytes: archive, memberPath: APP_BINARY_MEMBER });
  assert.equal(member.name, APP_BINARY_MEMBER);
  assert.equal(member.sha256, sha256Hex(binary));
  assert.deepEqual(member.bytes, binary);
  // `helper` ends with the same file name; a suffix match would return the wrong executable.
  assert.equal(readTarGzMember({ bytes: archive, memberPath: "MacOS/ferryx" }), null);
  assert.equal(readTarGzMember({ bytes: archive, memberPath: "Ferryx.app/Contents/MacOS/ferryx-x" }), null);
  // Corrupt gzip is an absence, not a throw.
  assert.equal(readTarGzMember({ bytes: Buffer.from("not gzip"), memberPath: APP_BINARY_MEMBER }), null);
});

test("the harness refuses the installed app and a missing binary", () => withTmp((root) => {
  const binary = join(root, "ferryx");
  writeFileSync(binary, "");
  assert.equal(assertSafeTarget({ binary }), binary);
  assert.throws(
    () => assertSafeTarget({ binary: "/Applications/Ferryx.app/Contents/MacOS/ferryx" }),
    (error) => error.code === "BINARY_IS_INSTALLED_APP",
  );
  assert.throws(() => assertSafeTarget({ binary: join(root, "absent") }), (error) => error.code === "BINARY_MISSING");
}));

test("a supplied --ips can never stand in for a forced crash", () => {
  // No --ips is the normal path: the run forces its own crash.
  assert.equal(assertReportSource({ ips: null, binary: "/tmp/ferryx" }), null);
  assert.throws(
    () => assertReportSource({ ips: "/tmp/old.ips", binary: "/tmp/ferryx" }),
    (error) => {
      assert.ok(error instanceof HarnessError);
      assert.equal(error.code, "IPS_NOT_ACCEPTABLE");
      // A usage refusal the operator fixes, not a host gap (exit 1, not 3).
      assert.equal(error.exit, 1);
      assert.match(error.message, /did not force/);
      return true;
    },
  );
  // A dry run only prints the plan, so naming a report there is harmless.
  assert.equal(assertReportSource({ ips: "/tmp/old.ips", binary: "/tmp/ferryx", dryRun: true }), "/tmp/old.ips");
});

test("the isolated environment points every location key inside one root", () => withTmp((root) => {
  const env = isolatedEnv(root, { PATH: "/usr/bin", FERRYX_RUNTIME_DIR: "/tmp/ferryx-501", FERRYX_DAEMON_SUPERVISOR_NAME: "com.ferryx." });
  for (const key of ["FERRYX_RUNTIME_DIR", "FERRYX_DATA_DIR", "FERRYX_SESSION_DIR", "HOME", "TMPDIR"]) {
    assert.ok(env[key].startsWith(root), `${key}=${env[key]} escapes ${root}`);
  }
  // Overrides win, so a production value inherited from the caller cannot leak into the crash lane.
  assert.notEqual(env.FERRYX_RUNTIME_DIR, "/tmp/ferryx-501");
  // No supervisor-name override: that would lift the isolation policy and install a real OS job.
  assert.equal(env.FERRYX_DAEMON_SUPERVISOR_NAME, undefined);
  assert.equal(env.PATH, "/usr/bin");
}));

test("symbolicatecrash is found through xcrun, then the framework paths, else absent", () => {
  const viaXcrun = findSymbolicatecrash({
    env: {},
    run: (command, args) =>
      command === "xcrun" && args[0] === "--find"
        ? { status: 0, stdout: "/Applications/Xcode.app/Contents/Developer/usr/bin/symbolicatecrash\n", stderr: "" }
        : { status: 1, stdout: "", stderr: "" },
  });
  assert.equal(viaXcrun, "/Applications/Xcode.app/Contents/Developer/usr/bin/symbolicatecrash");
  // An explicit override wins outright, so a host without Xcode can point at a copy.
  assert.equal(findSymbolicatecrash({ env: { SYMBOLICATECRASH: "/opt/sc" }, run: () => ({ status: 1, stdout: "", stderr: "" }) }), "/opt/sc");
  // Command Line Tools only: no xcrun hit, no framework copy, so the host gap is reported.
  assert.equal(findSymbolicatecrash({ env: {}, run: () => ({ status: 1, stdout: "", stderr: "" }) }), null);
});

/**
 * The measured Xcode layout this phase needed: on a current Xcode, `xcrun --find symbolicatecrash`
 * misses and the tool lives under `Contents/SharedFrameworks`, not the DTDeviceKitBase path the
 * harness used to try alone. Without these candidates the acceptance reported a host gap on a host
 * that has the tool.
 */
test("symbolicatecrash is found under Xcode's SharedFrameworks when xcrun misses it", () => {
  const developerDir = "/Applications/Xcode.app/Contents/Developer";
  const dtDeviceKit = join(
    developerDir,
    "Platforms/MacOSX.platform/Developer/Library/PrivateFrameworks/DTDeviceKitBase.framework/Versions/A/Resources/symbolicatecrash",
  );
  const sharedDvt = join(developerDir, "..", "SharedFrameworks", "DVTFoundation.framework", "Versions", "A", "Resources", "symbolicatecrash");
  const assetRuntime = join(
    developerDir,
    "..",
    "SharedFrameworks",
    "AssetRuntime",
    "DVTFoundation.framework",
    "Versions",
    "A",
    "Resources",
    "symbolicatecrash",
  );
  const run = (command) =>
    command === "xcode-select"
      ? { status: 0, stdout: `${developerDir}\n`, stderr: "" }
      : { status: 1, stdout: "", stderr: "" };

  // The reported failure: only the SharedFrameworks copy exists.
  const tried = [];
  assert.equal(
    findSymbolicatecrash({
      env: {},
      run,
      exists: (candidate) => {
        tried.push(candidate);
        return candidate === sharedDvt;
      },
    }),
    sharedDvt,
  );
  // Order is deterministic: the pre-existing DTDeviceKitBase path is still tried first, so a host
  // whose Xcode predates the move keeps resolving to the copy it has always used.
  assert.equal(tried[0], dtDeviceKit);
  // The two symbolicatecrash candidates in order; the last probe is CrashSymbolicator.py, resolved
  // from the same developer dir right after the text tool is found.
  assert.deepEqual(tried.slice(0, 2), [dtDeviceKit, sharedDvt]);
  assert.equal(tried.length, 3);
  assert.match(tried[2], /CoreSymbolicationDT\.framework\/Versions\/A\/Resources\/CrashSymbolicator\.py$/);

  // Both copies exist on a real host: the older location still wins, unchanged behaviour.
  assert.equal(findSymbolicatecrash({ env: {}, run, exists: (candidate) => candidate === dtDeviceKit || candidate === sharedDvt }), dtDeviceKit);

  // The AssetRuntime copy is the last resort, and DEVELOPER_DIR is honoured over xcode-select.
  assert.equal(
    findSymbolicatecrash({
      env: { DEVELOPER_DIR: developerDir },
      run: () => ({ status: 1, stdout: "", stderr: "" }),
      exists: (candidate) => candidate === assetRuntime,
    }),
    assetRuntime,
  );

  // A developer dir with none of the copies is still an absent tool, not a guessed path.
  assert.equal(findSymbolicatecrash({ env: { DEVELOPER_DIR: developerDir }, run, exists: () => false }), null);
});

/**
 * The measured failure: the tool was found and then exited 255 with
 * `Error: "DEVELOPER_DIR" is not defined at .../symbolicatecrash line 69`.
 *
 * Apple's script will not run unless `DEVELOPER_DIR` is in its environment, so the harness must
 * supply the dir it discovered the tool under. Both halves are pinned here: the dir is resolved
 * with the same precedence discovery uses, and it reaches the tool's `env`.
 */
test("symbolicatecrash is run with the DEVELOPER_DIR it requires", () => {
  const developerDir = "/Applications/Xcode.app/Contents/Developer";
  const selected = () => ({ status: 0, stdout: `${developerDir}\n`, stderr: "" });

  // An exported DEVELOPER_DIR wins; `xcode-select -p` is the fallback for the ordinary shell that
  // has Xcode selected but has never exported the variable.
  assert.equal(activeDeveloperDir({ env: { DEVELOPER_DIR: "/opt/xcode" }, run: selected }), "/opt/xcode");
  assert.equal(activeDeveloperDir({ env: {}, run: selected }), developerDir);
  // No selected developer dir at all: null rather than an empty string, so the caller can tell
  // "nothing to pass" from "pass an empty value".
  assert.equal(activeDeveloperDir({ env: {}, run: () => ({ status: 1, stdout: "", stderr: "" }) }), null);
  assert.equal(activeDeveloperDir({ env: {}, run: () => ({ status: 0, stdout: "\n", stderr: "" }) }), null);

  // Resolution returns the tool AND the dir together, so they cannot disagree.
  const resolved = resolveSymbolicatecrash({
    env: {},
    run: (command, args) =>
      command === "xcrun" && args[0] === "--find"
        ? { status: 0, stdout: `${developerDir}/usr/bin/symbolicatecrash\n`, stderr: "" }
        : selected(),
    // Pinned absent, so this test states the symbolicatecrash contract on every host rather than
    // depending on whether THIS machine has an Xcode with CoreSymbolicationDT.
    exists: () => false,
  });
  assert.deepEqual(resolved, { tool: `${developerDir}/usr/bin/symbolicatecrash`, developerDir, crashSymbolicator: null });

  // The environment handed to the tool carries the dir and keeps everything else, because the
  // script shells out to `xcrun`, `atos` and `dwarfdump` from PATH.
  const env = symbolicationEnv({ developerDir, base: { PATH: "/usr/bin", HOME: "/Users/x" } });
  assert.deepEqual(env, { PATH: "/usr/bin", HOME: "/Users/x", DEVELOPER_DIR: developerDir });
  // A host with no discoverable dir is passed through untouched rather than given an empty value.
  assert.deepEqual(symbolicationEnv({ developerDir: null, base: { PATH: "/usr/bin" } }), { PATH: "/usr/bin" });

  // The injected run seam sees it: what `symbolicate` actually invokes.
  const calls = [];
  symbolicate({
    tool: "/Applications/Xcode.app/usr/bin/symbolicatecrash",
    ips: "/tmp/crash.ips",
    dsym: "/tmp/ferryx.dSYM",
    env,
    run: (command, args, options) => {
      calls.push({ command, args, env: options?.env });
      return { status: 0, stdout: "", stderr: "" };
    },
  });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].env.DEVELOPER_DIR, developerDir);
  assert.deepEqual(calls[0].args, ["/tmp/crash.ips", "/tmp/ferryx.dSYM"]);
});

test("a mismatched dSYM is a REAL archive, and its absence is an environment gap", () => withTmp((root) => {
  const calls = [];
  const bundle = produceMismatchedDsym({
    outDir: root,
    binaryUuids: [ARM64],
    run: (command, args) => {
      calls.push([command, ...args]);
      if (command === "xcrun" && args[0] === "clang") return { status: 0, stdout: "", stderr: "" };
      if (command === "xcrun" && args[0] === "dsymutil") {
        mkdirSync(join(args[args.indexOf("-o") + 1], "Contents", "Resources", "DWARF"), { recursive: true });
        return { status: 0, stdout: "", stderr: "" };
      }
      // The fixture is read back so a UUID collision can never make the negative run vacuous.
      return { status: 0, stdout: `UUID: 4A1B2C3D-0000-1111-2222-333344445555 (arm64) ${args.at(-1)}\n`, stderr: "" };
    },
  });
  assert.ok(bundle.endsWith("mismatch.dSYM"));
  assert.deepEqual(calls[0].slice(0, 4), ["xcrun", "clang", "-g", "-O0"]);
  assert.throws(
    () => produceMismatchedDsym({ outDir: join(root, "x"), run: () => ({ status: 1, stdout: "", stderr: "no debug symbols" }) }),
    (error) => error instanceof EnvironmentGap && error.exit === 3,
  );
  // A fixture that happens to carry the binary's own UUID would make the negative run prove nothing.
  assert.throws(
    () =>
      produceMismatchedDsym({
        outDir: join(root, "y"),
        binaryUuids: [ARM64],
        run: (command, args) =>
          command === "dwarfdump"
            ? { status: 0, stdout: `UUID: ${ARM64} (arm64) ${args.at(-1)}\n`, stderr: "" }
            : command === "xcrun" && args[0] === "dsymutil"
              ? (mkdirSync(join(args.at(-1), "Contents", "Resources", "DWARF"), { recursive: true }), { status: 0, stdout: "", stderr: "" })
              : { status: 0, stdout: "", stderr: "" },
      }),
    (error) => error.code === "MISMATCH_DSYM_COLLIDES" && error.exit === 3,
  );
}));

test("crash report discovery is oldest-first and reads only .ips files", () => withTmp((root) => {
  const older = join(root, "a.ips");
  const newer = join(root, "b.ips");
  writeFileSync(older, "first");
  writeFileSync(newer, "second");
  // mtimestamps are explicit so the ordering assertion cannot pass on write-order luck.
  utimesSync(older, new Date("2026-11-16T00:00:00Z"), new Date("2026-11-16T00:00:00Z"));
  utimesSync(newer, new Date("2026-11-16T01:00:00Z"), new Date("2026-11-16T01:00:00Z"));
  writeFileSync(join(root, "notes.txt"), "ignored");
  assert.deepEqual(listCrashReports(root), [older, newer]);
  assert.deepEqual(listCrashReports(join(root, "absent")), []);
}));

/**
 * Measured on a real host: ReportCrash writes `.ferryx-<stamp>.ips` and renames it to
 * `ferryx-<stamp>.ips`. The watcher read the dot-file after the rename and died of ENOENT while the
 * final report sat next to it, so a hidden name is never a candidate.
 */
test("ReportCrash's hidden temp file is never a crash-report candidate", () => withTmp((root) => {
  const temp = join(root, ".ferryx-2026-11-19-185015.ips");
  const real = join(root, "ferryx-2026-11-19-185015.ips");
  // Only the temp exists at first: it names the UUID, so anything that reads dot-names claims it.
  writeFileSync(temp, MODERN_REPORT);
  assert.deepEqual(listCrashReports(root), []);
  const handlers = [];
  const watchImpl = (dir, options, handler) => {
    handlers.push(handler);
    return { close() {}, on() {} };
  };
  const pending = awaitNewCrashReport({ dir: root, before: [], uuids: [ARM64], deadlineAt: Date.now() + 5_000, watchImpl });
  // A watcher event while only the temp is there must not settle on it.
  handlers[0]();
  // The rename lands the final report next to a temp that is already gone.
  writeFileSync(real, MODERN_REPORT);
  handlers[0]();
  return pending.then((file) => {
    assert.equal(file, real);
    assert.deepEqual(listCrashReports(root), [real]);
  });
}));

/**
 * The same rename race, one step later: a name that WAS listed is gone by the read. That is not a
 * failure and not the deadline -- the wait skips it and keeps watching for the renamed file.
 */
test("a candidate that vanishes between listing and read is skipped, not fatal", () => withTmp((root) => {
  const vanished = join(root, "ferryx-temp.ips");
  const real = join(root, "ferryx-real.ips");
  writeFileSync(vanished, MODERN_REPORT);
  utimesSync(vanished, new Date("2026-11-19T00:00:00Z"), new Date("2026-11-19T00:00:00Z"));
  const handlers = [];
  const watchImpl = (dir, options, handler) => {
    handlers.push(handler);
    return { close() {}, on() {} };
  };
  const pending = awaitNewCrashReport({
    dir: root,
    before: [],
    uuids: [ARM64],
    deadlineAt: Date.now() + 5_000,
    watchImpl,
    // Exactly the rename the measured run lost to: the file is listed, then gone by the read. It is
    // removed for real so the next scan sees the same directory the harness did.
    readText: (file) => {
      if (file === vanished) {
        rmSync(file, { force: true });
        const error = new Error(`ENOENT: no such file or directory, open '${file}'`);
        error.code = "ENOENT";
        throw error;
      }
      return readFileSync(file, "utf8");
    },
  });
  // A watcher event with only the doomed candidate present must NOT resolve null: resolving here
  // would abandon the very report the rename is about to produce. The final assert below is what
  // proves it kept waiting -- a premature null would leave `pending` settled and unchangeable.
  handlers[0]();
  writeFileSync(real, MODERN_REPORT);
  handlers[0]();
  return pending.then((file) => assert.equal(file, real));
}));

/**
 * A host that has never crashed anything has no `DiagnosticReports` directory, and `fs.watch` on a
 * missing path throws ENOENT (the harness died that way before it ever reached a crash).
 */
test("an absent reports directory is created before it is watched", () => withTmp((root) => {
  const reportsDir = join(root, "Library", "Logs", "DiagnosticReports");
  assert.equal(existsSync(reportsDir), false);
  const watchedDirs = [];
  const handlers = [];
  const watchImpl = (dir, options, handler) => {
    // Recorded as the watcher saw it: the path handed to `fs.watch` must already exist.
    watchedDirs.push({ dir, exists: existsSync(dir) });
    handlers.push(handler);
    return { close() {}, on() {} };
  };
  const pending = awaitNewCrashReport({ dir: reportsDir, before: [], uuids: [ARM64], deadlineAt: Date.now() + 5_000, watchImpl });
  assert.equal(existsSync(reportsDir), true);
  assert.deepEqual(watchedDirs, [{ dir: reportsDir, exists: true }]);
  writeFileSync(join(reportsDir, "ferryx-2026-11-19-190000.ips"), MODERN_REPORT);
  handlers[0]();
  return pending.then((file) => {
    assert.match(file, /ferryx-2026-11-19-190000\.ips$/);
  });
}));

test("UUID matching accepts the dashed and undashed report forms", () => {
  assert.equal(reportNamesUuid(MODERN_REPORT, [ARM64]), true);
  assert.equal(reportNamesUuid('{"uuid":"883bc83eb8fb373e9a6c14c1ff47a2f6"}', [ARM64]), true);
  assert.equal(reportNamesUuid('{"uuid":"11111111-1111-1111-1111-111111111111"}', [ARM64]), false);
});

test("a crash report's pid and executable are read from JSON and legacy text", () => {
  const modern = crashReportIdentity(reportForPid({ pid: 4242, procPath: "/tmp/qa/ferryx" }));
  assert.deepEqual(modern.pids, [4242]);
  assert.deepEqual(modern.procPaths, ["/tmp/qa/ferryx"]);
  assert.deepEqual(modern.uuids, [ARM64]);
  const legacy = crashReportIdentity(
    [
      '{"app_name":"ferryx","bug_type":"309"}',
      "Process:               ferryx [43392]",
      "Path:                  /Users/x/ferryx",
      "Binary Images:",
      "       0x100000000 - 0x1009ffff +ferryx <883bc83e-b8fb-373e-9a6c-14c1ff47a2f6> /Users/x/ferryx",
    ].join("\n"),
  );
  assert.deepEqual(legacy.pids, [43392]);
  assert.deepEqual(legacy.procPaths, ["/Users/x/ferryx"]);
  assert.deepEqual(legacy.uuids, [ARM64]);
  assert.deepEqual(crashReportIdentity("").pids, []);
});

test("provenance requires a forced crash bound to the spawned pid AND its report", () => {
  // The complete record `acquireForcedCrash` emits. Every field below is one the evaluator reads:
  // a fixture that omitted the digest or the build UUIDs would be refused for the missing field
  // rather than for the clause the case is about, which is how these cases silently stopped
  // testing anything.
  const forced = {
    way: "forced-segv",
    pid: 4242,
    signal: "SIGSEGV",
    spawnedBinary: "/tmp/qa/ferryx",
    spawnedSha256: PRE_CRASH_SHA,
    sha256UnchangedAfterCrash: true,
    binaryUuids: [ARM64],
    spawnedAt: SPAWNED_AT,
    readyToken: "FERRYX_DAEMON_READY",
    readyAt: "2026-11-15T18:58:30.000Z",
    reportWrittenAfterCrash: true,
    reportTimestamp: REPORT_TIMESTAMP,
    reportWrittenAt: REPORT_WRITTEN_AT,
    reportPids: [4242],
    reportProcPaths: ["/tmp/qa/ferryx"],
    reportUuids: [ARM64],
  };
  assert.equal(evaluateForcedCrashProvenance({ forced, acceptedBinary: "/tmp/qa/ferryx" }).ok, true);

  // The `--ips` case: no forced record at all, so a supplied report can never qualify.
  const supplied = evaluateForcedCrashProvenance({ forced: null, acceptedBinary: "/tmp/qa/ferryx" });
  assert.equal(supplied.ok, false);
  assert.match(supplied.detail, /no forced crash was recorded/);

  // A report naming a DIFFERENT pid is not this run's crash, even though everything else looks right.
  const otherPid = evaluateForcedCrashProvenance({ forced: { ...forced, reportPids: [9999] }, acceptedBinary: "/tmp/qa/ferryx" });
  assert.equal(otherPid.ok, false);
  assert.match(otherPid.detail, /names pid\(s\) 9999/);

  // A report naming a different executable is refused too.
  const otherPath = evaluateForcedCrashProvenance({
    forced: { ...forced, reportProcPaths: ["/tmp/qa/other/ferryx"] },
    acceptedBinary: "/tmp/qa/ferryx",
  });
  assert.equal(otherPath.ok, false);
  assert.match(otherPath.detail, /not the binary this run spawned/);

  // And the ordinary refusals still hold.
  assert.equal(evaluateForcedCrashProvenance({ forced: { ...forced, signal: "SIGKILL" } }).ok, false);
  assert.equal(evaluateForcedCrashProvenance({ forced: { ...forced, readyToken: null } }).ok, false);
  assert.equal(evaluateForcedCrashProvenance({ forced: { ...forced, reportWrittenAfterCrash: false } }).ok, false);
  assert.equal(
    evaluateForcedCrashProvenance({ forced: { ...forced, spawnedBinary: "/tmp/other/ferryx" }, acceptedBinary: "/tmp/qa/ferryx" }).ok,
    false,
  );

  // --- the bytes binding -------------------------------------------------------------------
  // A record with no pre-crash digest describes no particular bytes, so nothing ties the
  // symbolicated image to the process that died.
  const noDigest = evaluateForcedCrashProvenance({ forced: { ...forced, spawnedSha256: null } });
  assert.equal(noDigest.ok, false);
  assert.match(noDigest.detail, /not captured before the crash/);

  // A digest that changed across the crash means the file on disk is not the one that ran, even
  // though the path still resolves and the report still names the pid.
  const movedBytes = evaluateForcedCrashProvenance({ forced: { ...forced, sha256UnchangedAfterCrash: false } });
  assert.equal(movedBytes.ok, false);
  assert.match(movedBytes.detail, /hash the same before and after the crash/);

  // --- the build-UUID binding -------------------------------------------------------------
  // No UUID read from the executable before the crash: the report's UUIDs cannot be compared
  // with anything, so a report for another build of the same name would pass.
  const noUuid = evaluateForcedCrashProvenance({ forced: { ...forced, binaryUuids: [] } });
  assert.equal(noUuid.ok, false);
  assert.match(noUuid.detail, /build UUID was not read before the crash/);

  // The report names a real ferryx image, but a different link: this is a different build's crash
  // wearing this binary's name, which is exactly what the UUID comparison exists to catch.
  const foreignBuild = evaluateForcedCrashProvenance({ forced: { ...forced, reportUuids: [X86] } });
  assert.equal(foreignBuild.ok, false);
  assert.match(foreignBuild.detail, /none of which is the spawned binary's/);

  // A universal binary carries two UUIDs and the report names the x86_64 slice; that IS this
  // binary, so the overlap rule must accept a partial match rather than demand equality.
  assert.equal(
    evaluateForcedCrashProvenance({ forced: { ...forced, binaryUuids: [ARM64, X86], reportUuids: [X86] } }).ok,
    true,
  );

  // --- the ordering binding ----------------------------------------------------------------
  // A report dated before the spawn is a historical crash handed in afterwards. The comparison is
  // against the report's own normalized header time, not its file mtime.
  const predatesSpawn = evaluateForcedCrashProvenance({
    forced: { ...forced, reportWrittenAt: "2026-11-15T18:00:00.000Z" },
  });
  assert.equal(predatesSpawn.ok, false);
  assert.match(predatesSpawn.detail, /before this run spawned the binary/);

  // An unrecorded spawn time leaves nothing to order the report against, so it is refused rather
  // than assumed ordered.
  const noSpawnTime = evaluateForcedCrashProvenance({ forced: { ...forced, spawnedAt: null } });
  assert.equal(noSpawnTime.ok, false);
  assert.match(noSpawnTime.detail, /spawn time was not recorded/);
});

/**
 * The measured failure: macOS writes the user's home in an `.ips` path as `/Users/USER/<*>/...`.
 *
 * The provenance check compares executables exactly, so a redacted path was reported as "not the
 * binary this run spawned" on a crash that was in fact the right one. The fix reconciles a
 * redacted path with the spawned one BEFORE that strict comparison, and only when the redaction is
 * consistent: the report must carry the marker, the spawned path must sit under the real home, and
 * the kept tail must match exactly. Everything else still fails, and pid/UUID clauses are untouched.
 */
test("a home-redacted report path is matched only when it is consistent with the spawned binary", () => {
  const home = "/Users/I552267";
  const spawned = `${home}/code/ferryx-wt/.evidence/phase0/bin/pkg/Ferryx.app/Contents/MacOS/ferryx`;
  const redacted = `${REDACTED_HOME_PREFIX}*/Ferryx.app/Contents/MacOS/ferryx`;

  // Consistent redaction: the tail after `*` is exactly the spawned path's tail.
  const match = matchesRedactedProcPath({ reported: redacted, spawned, home });
  assert.ok(match, "a consistent redaction must be recognized");
  assert.equal(match.tail, "/Ferryx.app/Contents/MacOS/ferryx");
  assert.equal(match.spawned, spawned);

  // A redaction that keeps a DIFFERENT tail names a different file, so it is not a match. The
  // `Ferryx.app` bundle name alone cannot be the whole comparison.
  assert.equal(
    matchesRedactedProcPath({ reported: `${REDACTED_HOME_PREFIX}*/Other.app/Contents/MacOS/ferryx`, spawned, home }),
    null,
  );
  assert.equal(
    matchesRedactedProcPath({ reported: `${REDACTED_HOME_PREFIX}*/Ferryx.app/Contents/MacOS/helper`, spawned, home }),
    null,
  );
  // A redaction with no tail preserves nothing to compare.
  assert.equal(matchesRedactedProcPath({ reported: `${REDACTED_HOME_PREFIX}*`, spawned, home }), null);
  assert.equal(matchesRedactedProcPath({ reported: REDACTED_HOME_PREFIX, spawned, home }), null);

  // The spawned path must be under the real home: a redaction cannot excuse a binary outside it.
  assert.equal(
    matchesRedactedProcPath({ reported: redacted, spawned: `/opt/qa/Ferryx.app/Contents/MacOS/ferryx`, home }),
    null,
  );
  // A sibling directory that merely shares the home's prefix is not under the home.
  assert.equal(
    matchesRedactedProcPath({
      reported: redacted,
      spawned: `/Users/I5522670/Ferryx.app/Contents/MacOS/ferryx`,
      home,
    }),
    null,
  );
});

test("provenance accepts a redacted-consistent path and still refuses a mismatching one", () => {
  const home = "/Users/I552267";
  const spawned = `${home}/code/ferryx-wt/.evidence/phase0/bin/pkg/Ferryx.app/Contents/MacOS/ferryx`;
  const redacted = `${REDACTED_HOME_PREFIX}*/Ferryx.app/Contents/MacOS/ferryx`;

  const forced = {
    way: "forced-segv",
    pid: 51269,
    signal: "SIGSEGV",
    spawnedBinary: spawned,
    spawnedSha256: PRE_CRASH_SHA,
    sha256UnchangedAfterCrash: true,
    binaryUuids: [ARM64],
    spawnedAt: SPAWNED_AT,
    readyToken: "FERRYX_DAEMON_READY",
    readyAt: "2026-11-15T18:58:30.000Z",
    reportWrittenAfterCrash: true,
    reportTimestamp: REPORT_TIMESTAMP,
    reportWrittenAt: REPORT_WRITTEN_AT,
    reportPids: [51269],
    reportProcPaths: [redacted],
    reportUuids: [ARM64],
  };

  // 1. Redacted and consistent: passes, and the reconciliation is reported rather than silent.
  const reconciled = reconcileReportProcPaths({ forced, acceptedBinary: spawned, home });
  assert.deepEqual(reconciled.forced.reportProcPaths, [spawned]);
  assert.deepEqual(reconciled.matched, [
    { reported: redacted, spawned, tail: "/Ferryx.app/Contents/MacOS/ferryx", home },
  ]);
  // The record handed in is not mutated: the evidence keeps what the OS actually wrote.
  assert.deepEqual(forced.reportProcPaths, [redacted]);
  const accepted = evaluateForcedCrashProvenance({ forced: reconciled.forced, acceptedBinary: spawned });
  assert.equal(accepted.ok, true, accepted.detail);

  // 2. Redacted with a DIFFERENT tail: not reconciled, so the strict clause still refuses it.
  const wrongTail = { ...forced, reportProcPaths: [`${REDACTED_HOME_PREFIX}*/Other.app/Contents/MacOS/ferryx`] };
  const wrongTailReconciled = reconcileReportProcPaths({ forced: wrongTail, acceptedBinary: spawned, home });
  assert.deepEqual(wrongTailReconciled.matched, []);
  const wrongTailVerdict = evaluateForcedCrashProvenance({
    forced: wrongTailReconciled.forced,
    acceptedBinary: spawned,
  });
  assert.equal(wrongTailVerdict.ok, false);
  assert.match(wrongTailVerdict.detail, /not the binary this run spawned/);

  // 3. Unredacted and exact: passes untouched, with nothing recorded as redacted.
  const exact = reconcileReportProcPaths({ forced: { ...forced, reportProcPaths: [spawned] }, acceptedBinary: spawned, home });
  assert.deepEqual(exact.matched, []);
  assert.equal(evaluateForcedCrashProvenance({ forced: exact.forced, acceptedBinary: spawned }).ok, true);

  // 4. Unredacted and different: refused, exactly as before the redaction rule existed.
  const otherUser = `/Users/someone-else/Ferryx.app/Contents/MacOS/ferryx`;
  const otherReconciled = reconcileReportProcPaths({ forced: { ...forced, reportProcPaths: [otherUser] }, acceptedBinary: spawned, home });
  assert.deepEqual(otherReconciled.matched, []);
  const otherVerdict = evaluateForcedCrashProvenance({ forced: otherReconciled.forced, acceptedBinary: spawned });
  assert.equal(otherVerdict.ok, false);
  assert.match(otherVerdict.detail, /not the binary this run spawned/);

  // The redaction only rewrites the PATH clause: the pid and build-UUID bindings still refuse.
  assert.equal(
    evaluateForcedCrashProvenance({ forced: { ...reconciled.forced, reportPids: [9999] }, acceptedBinary: spawned }).ok,
    false,
  );
  assert.equal(
    evaluateForcedCrashProvenance({ forced: { ...reconciled.forced, reportUuids: [X86] }, acceptedBinary: spawned }).ok,
    false,
  );
});

test("the forced-crash record states the identity the report carries", () => {
  // `outcome` is what `forceSegv` resolves plus the four observations `acquireForcedCrash` adds
  // from its own reads: the pre-crash digest, whether it survived the crash, the executable's
  // build UUIDs, and the spawn instant. All four are asserted below, because a record that loses
  // one is refused by the provenance check and the summary would then understate the evidence.
  const record = buildForcedCrashRecord({
    outcome: {
      pid: 4242,
      signal: "SIGSEGV",
      spawnedBinary: "/tmp/qa/ferryx",
      spawnedSha256: PRE_CRASH_SHA,
      sha256UnchangedAfterCrash: true,
      binaryUuids: [ARM64],
      spawnedAt: SPAWNED_AT,
      readyToken: "FERRYX_DAEMON_READY",
      readyAt: "2026-11-16T04:00:00.000Z",
    },
    crashText: reportForPid({ pid: 4242, procPath: "/tmp/qa/ferryx" }),
    reportPath: "/tmp/reports/ferryx.ips",
    reportWrittenAfterCrash: true,
  });
  assert.equal(record.way, "forced-segv");
  assert.equal(record.pid, 4242);
  assert.equal(record.signal, "SIGSEGV");
  assert.equal(record.spawnedBinary, "/tmp/qa/ferryx");
  assert.equal(record.reportPath, "/tmp/reports/ferryx.ips");
  assert.deepEqual(record.reportPids, [4242]);
  assert.deepEqual(record.reportProcPaths, ["/tmp/qa/ferryx"]);
  assert.deepEqual(record.reportUuids, [ARM64]);
  assert.equal(record.reportWrittenAfterCrash, true);
  // The bytes binding travels with the record, not only with the identity object.
  assert.equal(record.spawnedSha256, PRE_CRASH_SHA);
  assert.equal(record.sha256UnchangedAfterCrash, true);
  assert.deepEqual(record.binaryUuids, [ARM64]);
  assert.equal(record.spawnedAt, SPAWNED_AT);
  // The header timestamp is normalized, so the ordering check compares instants and not strings:
  // `04:00 +0900` is 19:00Z, and asserting the raw string alone would hide a wrong conversion.
  assert.equal(record.reportTimestamp, REPORT_TIMESTAMP);
  assert.equal(record.reportWrittenAt, REPORT_WRITTEN_AT);
  // The record this test just built is what the gate reads, so it must satisfy the check.
  const verdict = evaluateForcedCrashProvenance({ forced: record, acceptedBinary: "/tmp/qa/ferryx" });
  assert.equal(verdict.ok, true, verdict.detail);
  assert.equal(buildForcedCrashRecord({}), null);
});

test("the forced-crash record carries no digest when the outcome has none", () => {
  // A caller that could not read the bytes before spawning must not be given a digest anyway: a
  // fabricated or post-hoc one would be the very weakness the pre-crash read exists to remove.
  const record = buildForcedCrashRecord({
    outcome: { pid: 4242, signal: "SIGSEGV", spawnedBinary: "/tmp/qa/ferryx", readyToken: "FERRYX_DAEMON_READY" },
    crashText: reportForPid({ pid: 4242, procPath: "/tmp/qa/ferryx" }),
    reportWrittenAfterCrash: true,
  });
  assert.equal(record.spawnedSha256, null);
  assert.equal(record.sha256UnchangedAfterCrash, false);
  assert.equal(evaluateForcedCrashProvenance({ forced: record }).ok, false);
});

test("packaged identity needs a digest AND the build UUIDs, from the tarball member", () => {
  const binary = macho();
  const packaged = { artifact: { path: "/tmp/Ferryx.app.tar.gz", memberPath: APP_BINARY_MEMBER }, sha256: sha256Hex(binary), uuids: [ARM64] };
  // The accepted side is the shape `buildBinaryIdentity` produces: a digest read BEFORE the crash,
  // plus the flag saying it was re-checked after. Without both flags the check refuses the record
  // no matter how well the digests agree, so a fixture that omitted them would assert a pass the
  // evaluator never gives.
  const accepted = {
    path: "/tmp/ferryx",
    sha256: sha256Hex(binary),
    uuids: [ARM64],
    capturedBeforeCrash: true,
    verifiedUnchangedAfterCrash: true,
  };
  assert.equal(evaluatePackagedBinaryIdentity({ accepted, packaged }).ok, true);

  // Different bytes: the accepted binary is not the shipped one.
  const otherBytes = evaluatePackagedBinaryIdentity({ accepted, packaged: { ...packaged, sha256: sha256Hex(Buffer.from("x")) } });
  assert.equal(otherBytes.ok, false);
  assert.match(otherBytes.detail, /different bytes/);

  // Same digest but no UUID is refused: a hash alone does not prove the build identity.
  assert.equal(evaluatePackagedBinaryIdentity({ accepted, packaged: { ...packaged, uuids: [] } }).ok, false);
  // Right digest, wrong UUID: a stale artifact for a rebuilt binary.
  assert.equal(evaluatePackagedBinaryIdentity({ accepted, packaged: { ...packaged, uuids: [X86] } }).ok, false);
  // No artifact read at all: unproven, never a pass.
  assert.equal(evaluatePackagedBinaryIdentity({ accepted, packaged: null }).ok, false);
  // The artifact and its member must be named, or the check cannot be re-run.
  assert.equal(evaluatePackagedBinaryIdentity({ accepted, packaged: { ...packaged, artifact: {} } }).ok, false);

  // A digest taken after the fact is not the acceptance-time digest, even when it matches the
  // packaged bytes exactly: the file could have been replaced between the crash and the hash.
  const lateDigest = evaluatePackagedBinaryIdentity({
    accepted: { ...accepted, capturedBeforeCrash: false },
    packaged,
  });
  assert.equal(lateDigest.ok, false);
  assert.match(lateDigest.detail, /not captured before the crash/);

  // And a digest never re-checked cannot show the executable survived the run.
  const unchecked = evaluatePackagedBinaryIdentity({
    accepted: { ...accepted, verifiedUnchangedAfterCrash: false },
    packaged,
  });
  assert.equal(unchecked.ok, false);
  assert.match(unchecked.detail, /not re-checked after the crash/);
});

test("buildBinaryIdentity states the pre-crash digest and the post-crash re-check", () => {
  const bytes = macho();
  // Same bytes before and after: the digest is the pre-image's, and the re-check agrees.
  const unchanged = buildBinaryIdentity({ path: "/tmp/ferryx", before: bytes, after: Buffer.from(bytes) });
  assert.equal(unchanged.sha256, sha256Hex(bytes));
  assert.equal(unchanged.sha256AfterCrash, sha256Hex(bytes));
  assert.equal(unchanged.capturedBeforeCrash, true);
  assert.equal(unchanged.verifiedUnchangedAfterCrash, true);
  assert.deepEqual(unchanged.uuids, [ARM64]);
  assert.equal(unchanged.bytes, bytes.length);
  assert.equal(resolve(unchanged.path), "/tmp/ferryx");

  // The executable was swapped while the run was in flight: the published digest must stay the
  // pre-crash one (it is what died), while the re-check records that it no longer describes disk.
  const swapped = buildBinaryIdentity({ path: "/tmp/ferryx", before: bytes, after: macho({ uuid: X86 }) });
  assert.equal(swapped.sha256, sha256Hex(bytes));
  assert.notEqual(swapped.sha256AfterCrash, swapped.sha256);
  assert.equal(swapped.verifiedUnchangedAfterCrash, false);

  // No post-image read at all is "unverified", never "unchanged".
  const unread = buildBinaryIdentity({ path: "/tmp/ferryx", before: bytes });
  assert.equal(unread.sha256AfterCrash, null);
  assert.equal(unread.verifiedUnchangedAfterCrash, false);
});

test("binary identity is refused unless the digest is pre-crash and agrees with the crash", () => {
  const bytes = macho();
  const identity = buildBinaryIdentity({ path: "/tmp/ferryx", before: bytes, after: Buffer.from(bytes) });
  const forced = { spawnedBinary: "/tmp/ferryx", spawnedSha256: sha256Hex(bytes) };
  assert.equal(evaluateBinaryIdentity({ binary: identity, forced }).ok, true);

  // No digest: the release gate has nothing to compare with the shipped artifact.
  const noDigest = evaluateBinaryIdentity({ binary: { ...identity, sha256: null }, forced });
  assert.equal(noDigest.ok, false);
  assert.match(noDigest.detail, /no binary\.sha256/);

  // The digest disagrees with the executable the crashed process was spawned from: the report
  // describes some other set of bytes than the one that died.
  const otherBytes = evaluateBinaryIdentity({
    binary: { ...identity, sha256: sha256Hex(Buffer.from("different")) },
    forced,
  });
  assert.equal(otherBytes.ok, false);
  assert.match(otherBytes.detail, /does not describe the bytes that died/);

  // A matching digest for a different path cannot ride on the path the crash ran.
  const otherPath = evaluateBinaryIdentity({
    binary: { ...identity, path: "/tmp/qa/other/ferryx" },
    forced,
  });
  assert.equal(otherPath.ok, false);
  assert.match(otherPath.detail, /but the crashed process ran/);

  // No forced crash at all: the digest is not tied to a process this run signalled.
  assert.equal(evaluateBinaryIdentity({ binary: identity, forced: null }).ok, false);
});

test("only the dSYM bundles carrying the crash UUID are symbolicated", () => {
  const resolved = resolveSymbolicationBundles({
    bundles: [
      { path: "/tmp/dsyms/ferryx.dSYM", uuids: [ARM64] },
      { path: "/tmp/dsyms/keeper.dSYM", uuids: [X86] },
      { path: "/tmp/dsyms/other.dSYM", uuids: ["99999999-9999-9999-9999-999999999999"] },
    ],
    uuids: [ARM64],
  });
  assert.deepEqual(resolved.paths, ["/tmp/dsyms/ferryx.dSYM"]);
  assert.deepEqual(resolved.missing, []);
  // A universal binary needs both bundles, and the missing one is named rather than dropped.
  const both = resolveSymbolicationBundles({
    bundles: [{ path: "/tmp/dsyms/ferryx.dSYM", uuids: [ARM64] }, { path: "/tmp/dsyms/keeper.dSYM", uuids: [X86] }],
    uuids: [ARM64, X86],
  });
  assert.deepEqual(both.paths, ["/tmp/dsyms/ferryx.dSYM", "/tmp/dsyms/keeper.dSYM"]);
  const gap = resolveSymbolicationBundles({ bundles: [{ path: "/tmp/d/f.dSYM", uuids: [ARM64] }], uuids: [ARM64, X86] });
  assert.deepEqual(gap.missing, [X86]);
  assert.deepEqual(resolveSymbolicationBundles({}).paths, []);
});

test("the crash-report wait claims only a NEW report naming the target UUID", () => withTmp((root) => {
  const before = join(root, "existing.ips");
  writeFileSync(before, MODERN_REPORT);
  const handlers = [];
  const watchImpl = (dir, options, handler) => {
    handlers.push(handler);
    return { close() {}, on() {} };
  };
  const pending = awaitNewCrashReport({
    dir: root,
    before: [before],
    uuids: [ARM64],
    deadlineAt: Date.now() + 5_000,
    watchImpl,
  });
  // A report that existed before the crash must never be claimed, even though it names the UUID.
  assert.equal(handlers.length, 1);
  // Now the crash writes its own report; the watcher callback is the only trigger.
  writeFileSync(join(root, "ferryx-2026-11-16.ips"), MODERN_REPORT);
  handlers[0]();
  return pending.then((file) => assert.match(file, /ferryx-2026-11-16\.ips$/));
}));

test("a report for another app never satisfies the wait", () => withTmp((root) => {
  const watchImpl = () => ({ close() {}, on() {} });
  writeFileSync(join(root, "Safari-2026-11-16.ips"), '{"usedImages":[{"name":"Safari","uuid":"11111111-1111-1111-1111-111111111111"}]}');
  return awaitNewCrashReport({
    dir: root,
    before: [],
    uuids: [ARM64],
    deadlineAt: Date.now() - 1,
    watchImpl,
  }).then((file) => assert.equal(file, null));
}));

test("the collector is invoked as `daemon crash-report` and its JSON is returned", () => {
  const calls = [];
  const collected = collectBundle({
    binary: "/tmp/ferryx",
    ips: "/tmp/a.ips",
    dsym: "/tmp/dsyms",
    out: "/tmp/bundle",
    run: (command, args) => {
      calls.push([command, ...args]);
      return { status: 0, stdout: JSON.stringify({ reportDir: "/tmp/bundle", dsym: "/tmp/dsyms/ferryx.dSYM" }), stderr: "" };
    },
  });
  assert.deepEqual(calls[0], ["/tmp/ferryx", "daemon", "crash-report", "--ips", "/tmp/a.ips", "--dsym", "/tmp/dsyms", "--out", "/tmp/bundle", "--json"]);
  assert.equal(collected.dsym, "/tmp/dsyms/ferryx.dSYM");
  assert.throws(
    () => collectBundle({ binary: "/tmp/ferryx", ips: "/tmp/a.ips", dsym: "/tmp/d", out: "/tmp/b", run: () => ({ status: 1, stdout: "", stderr: "boom" }) }),
    (error) => error instanceof HarnessError && error.code === "CRASH_REPORT_FAILED" && error.exit === 1,
  );
  assert.throws(
    () => collectBundle({ binary: "/tmp/ferryx", ips: "/tmp/a.ips", dsym: "/tmp/d", out: "/tmp/b", run: () => ({ status: 0, stdout: "not json", stderr: "" }) }),
    (error) => error.code === "CRASH_REPORT_UNREADABLE",
  );
});

test("the collector runs in the isolated environment the crash ran in", () => {
  // The crashed daemon's log lives under the isolated root, not the operator's install: without
  // this env the bundle's `daemon.log` would belong to some other process entirely.
  const env = { FERRYX_DATA_DIR: "/tmp/qa-root/data", HOME: "/tmp/qa-root/home" };
  let seen = null;
  collectBundle({
    binary: "/tmp/ferryx",
    ips: "/tmp/a.ips",
    dsym: "/tmp/dsyms",
    out: "/tmp/bundle",
    env,
    run: (command, args, options = {}) => {
      seen = options.env ?? null;
      return { status: 0, stdout: JSON.stringify({ reportDir: "/tmp/bundle" }), stderr: "" };
    },
  });
  assert.equal(seen, env);
  // With no env the collector keeps whatever environment the harness itself inherited.
  let inherited = "unset";
  collectBundle({
    binary: "/tmp/ferryx",
    ips: "/tmp/a.ips",
    dsym: "/tmp/dsyms",
    out: "/tmp/bundle",
    run: (command, args, options = {}) => {
      inherited = options.env ?? null;
      return { status: 0, stdout: JSON.stringify({}), stderr: "" };
    },
  });
  assert.equal(inherited, null);
});

/** A gate result shaped like `assertDsymGate`'s, for driving the judge without a real dSYM. */
function fakeGate() {
  return { binaryPath: "/tmp/ferryx", binaryUuids: [ARM64], bundles: [{ path: "/tmp/dsyms/ferryx.dSYM", uuids: [ARM64] }] };
}

/**
 * The regression this phase needed: the isolated root holds the crashed daemon's own log, so it
 * must still exist when the collector runs. Removing it first (the old `finally` around `forceSegv`)
 * made the bundle carry the operator's live-install log instead of the crash under test.
 */
test("a forced crash keeps its isolated root alive for the collector", () => withTmp(async (root) => {
  const reportsDir = join(root, "reports");
  mkdirSync(reportsDir);
  const qaRoot = join(root, "qa");
  const child = fakeChild();
  // A real file with real Mach-O bytes: `acquireForcedCrash` hashes the executable BEFORE spawning
  // and reads its build UUID from the same bytes, so a path that does not exist can only fail with
  // ENOENT -- which would test nothing about crashes, roots, or evidence.
  const binaryPath = join(root, "ferryx");
  const binaryBytes = macho();
  writeFileSync(binaryPath, binaryBytes);
  const deps = {
    reportsDir,
    // The spawn instant is fixed so the report timestamp below can be proven to follow it.
    now: () => CLOCK_MS,
    makeRoot: () => {
      mkdirSync(qaRoot, { recursive: true });
      return qaRoot;
    },
    // The OS writes the report after the crash; the watcher callback is the only trigger. The
    // report names the spawned pid and executable, as ReportCrash really does.
    watchImpl: (dir, options, handler) => {
      writeFileSync(
        join(dir, "ferryx-new.ips"),
        reportForPid({ pid: 4242, procPath: binaryPath, timestamp: REPORT_AFTER_CLOCK }),
      );
      queueMicrotask(handler);
      return { close() {}, on() {} };
    },
    spawnImpl: () => {
      queueMicrotask(() => {
        child.stdout.write("FERRYX_DAEMON_READY\n");
        child.emit("exit", null, "SIGSEGV");
      });
      return child;
    },
  };
  const acquired = await acquireForcedCrash({ binary: binaryPath, gate: { binaryUuids: [ARM64] }, deps });
  try {
    assert.equal(acquired.root, qaRoot);
    assert.match(acquired.crashPath, /ferryx-new\.ips$/);
    assert.equal(acquired.forced.signal, "SIGSEGV");
    // The record is bound to the spawn AND to the report the OS wrote afterwards.
    assert.equal(acquired.forced.pid, 4242);
    assert.equal(acquired.forced.spawnedBinary, binaryPath);
    assert.equal(acquired.forced.readyToken, "FERRYX_DAEMON_READY");
    assert.deepEqual(acquired.forced.reportPids, [4242]);
    assert.deepEqual(acquired.forced.reportProcPaths, [binaryPath]);
    assert.equal(acquired.forced.reportWrittenAfterCrash, true);
    // The bytes binding is part of the record, and it is the digest of the file that exists here.
    assert.equal(acquired.forced.spawnedSha256, sha256Hex(binaryBytes));
    assert.equal(acquired.forced.sha256UnchangedAfterCrash, true);
    assert.deepEqual(acquired.forced.binaryUuids, [ARM64]);
    assert.deepEqual(acquired.forced.reportUuids, [ARM64]);
    // The signal deliveries are on the record too: how many SIGSEGVs this run had to send (and when)
    // is evidence, not an implementation detail of the retry.
    assert.equal(acquired.forced.signalCount, 1);
    assert.equal(acquired.forced.signalDeliveries.length, 1);
    assert.equal(acquired.forced.signalDeliveries[0].pid, 4242);
    assert.equal(acquired.forced.signalDeliveries[0].signal, "SIGSEGV");
    // And that record passes the provenance check the release gate reads.
    const verdict = evaluateForcedCrashProvenance({ forced: acquired.forced, acceptedBinary: binaryPath });
    assert.equal(verdict.ok, true, verdict.detail);
    // Still on disk: the caller collects from it, then removes it.
    assert.ok(existsSync(acquired.root));
    // The collector environment points INTO that root, which is what makes the log tail the crash's.
    assert.ok(acquired.env.FERRYX_DATA_DIR.startsWith(acquired.root));
    assert.ok(acquired.env.HOME.startsWith(acquired.root));
  } finally {
    rmSync(acquired.root, { recursive: true, force: true });
  }
}));

test("a forced crash that leaves no report releases its root and names the gap", () => withTmp(async (root) => {
  const reportsDir = join(root, "reports");
  mkdirSync(reportsDir);
  const qaRoot = join(root, "qa");
  const child = fakeChild();
  const binaryPath = join(root, "ferryx");
  writeFileSync(binaryPath, macho());
  await assert.rejects(
    acquireForcedCrash({
      binary: binaryPath,
      gate: { binaryUuids: [ARM64] },
      deps: {
        reportsDir,
        makeRoot: () => {
          mkdirSync(qaRoot, { recursive: true });
          return qaRoot;
        },
        watchImpl: () => ({ close() {}, on() {} }),
        // Already past the deadline: the wait resolves null without a timer.
        now: () => 10_000,
        reportDeadlineMs: 1,
        spawnImpl: () => {
          queueMicrotask(() => {
            child.stdout.write("FERRYX_DAEMON_READY\n");
            child.emit("exit", null, "SIGSEGV");
          });
          return child;
        },
      },
    }),
    (error) => {
      assert.ok(error instanceof EnvironmentGap);
      assert.equal(error.code, "NO_CRASH_REPORT");
      assert.equal(error.exit, 3);
      return true;
    },
  );
  // No crash, no evidence: the root does not survive a run that produced nothing.
  assert.equal(existsSync(qaRoot), false);
}));

test("a report naming another pid does not become this run's forced crash", () => withTmp(async (root) => {
  const reportsDir = join(root, "reports");
  mkdirSync(reportsDir);
  const qaRoot = join(root, "qa");
  const child = fakeChild();
  const binaryPath = join(root, "ferryx");
  writeFileSync(binaryPath, macho());
  const acquired = await acquireForcedCrash({
    binary: binaryPath,
    gate: { binaryUuids: [ARM64] },
    deps: {
      reportsDir,
      now: () => CLOCK_MS,
      makeRoot: () => {
        mkdirSync(qaRoot, { recursive: true });
        return qaRoot;
      },
      // Some other process's crash, naming the same UUID: the report exists, but not for our pid.
      watchImpl: (dir, options, handler) => {
        writeFileSync(
          join(dir, "ferryx-other.ips"),
          reportForPid({ pid: 9999, procPath: binaryPath, timestamp: REPORT_AFTER_CLOCK }),
        );
        queueMicrotask(handler);
        return { close() {}, on() {} };
      },
      spawnImpl: () => {
        queueMicrotask(() => {
          child.stdout.write("FERRYX_DAEMON_READY\n");
          child.emit("exit", null, "SIGSEGV");
        });
        return child;
      },
    },
  });
  try {
    // The acquisition itself succeeds (a matching report was found), but the record shows the pid
    // mismatch, so the release gate refuses it. That is exactly the `--ips` gap being closed.
    assert.deepEqual(acquired.forced.reportPids, [9999]);
    // `acceptedBinary` matches what was spawned, so the refusal can only be the pid mismatch: a
    // stale path here would fail the binary clause and hide whether the pid binding still works.
    const verdict = evaluateForcedCrashProvenance({ forced: acquired.forced, acceptedBinary: binaryPath });
    assert.equal(verdict.ok, false);
    assert.match(verdict.detail, /pid\(s\) 9999/);
  } finally {
    rmSync(acquired.root, { recursive: true, force: true });
  }
}));

test("a forced crash creates a reports directory the host never had", () => withTmp(async (root) => {
  // The measured failure: a host that had never crashed anything has no DiagnosticReports, so the
  // snapshot listed nothing, `fs.watch` threw ENOENT, and ReportCrash had nowhere to write.
  const reportsDir = join(root, "Library", "Logs", "DiagnosticReports");
  const qaRoot = join(root, "qa");
  const child = fakeChild();
  const binaryPath = join(root, "ferryx");
  writeFileSync(binaryPath, macho());
  assert.equal(existsSync(reportsDir), false);
  const acquired = await acquireForcedCrash({
    binary: binaryPath,
    gate: { binaryUuids: [ARM64] },
    deps: {
      reportsDir,
      now: () => CLOCK_MS,
      makeRoot: () => {
        mkdirSync(qaRoot, { recursive: true });
        return qaRoot;
      },
      // Writing here only succeeds if the directory was created before the watch: this is the
      // assertion, not a fixture convenience.
      watchImpl: (dir, options, handler) => {
        writeFileSync(
          join(dir, "ferryx-first-crash.ips"),
          reportForPid({ pid: 4242, procPath: binaryPath, timestamp: REPORT_AFTER_CLOCK }),
        );
        queueMicrotask(handler);
        return { close() {}, on() {} };
      },
      spawnImpl: () => {
        queueMicrotask(() => {
          child.stdout.write("FERRYX_DAEMON_READY\n");
          child.emit("exit", null, "SIGSEGV");
        });
        return child;
      },
    },
  });
  try {
    assert.equal(existsSync(reportsDir), true);
    assert.match(acquired.crashPath, /ferryx-first-crash\.ips$/);
    assert.deepEqual(acquired.forced.reportPids, [4242]);
  } finally {
    rmSync(acquired.root, { recursive: true, force: true });
  }
}));

test("the packaged artifact is read from the tarball: digest plus build UUIDs", () => withTmp((root) => {
  const binaryBytes = macho();
  const artifact = join(root, "Ferryx.app.tar.gz");
  writeFileSync(artifact, tarGz([{ name: `./${APP_BINARY_MEMBER}`, bytes: binaryBytes }]));
  const inspected = inspectPackagedArtifact({ artifactPath: artifact });
  assert.equal(inspected.artifact.memberPath, APP_BINARY_MEMBER);
  assert.equal(inspected.sha256, sha256Hex(binaryBytes));
  assert.deepEqual(inspected.uuids, [ARM64]);
  assert.equal(inspected.problem, null);
  // The tarball's own digest is recorded, so the evidence names which artifact it describes.
  assert.equal(inspected.artifact.sha256, sha256Hex(readFileSync(artifact)));

  // A tarball without the executable is a recorded problem, not a silent empty pass.
  const wrong = join(root, "other.tar.gz");
  writeFileSync(wrong, tarGz([{ name: "./Ferryx.app/Contents/MacOS/helper", bytes: binaryBytes }]));
  const missing = inspectPackagedArtifact({ artifactPath: wrong });
  assert.equal(missing.sha256, null);
  assert.match(missing.problem, /holds no member/);
  // And an absent artifact is the same class of finding.
  assert.match(inspectPackagedArtifact({ artifactPath: join(root, "absent.tar.gz") }).problem, /no artifact/);
}));

/**
 * A runner covering every external command the judge sequence issues: the collector, the
 * mismatched-dSYM fixture, the gate, and symbolicatecrash. `dwarfdump` answers per path so the
 * negative run really mismatches, which is what the gate must refuse.
 */
function judgeRun({ collector, positive, negative, calls = [], envs = [] }) {
  return (command, args, options = {}) => {
    calls.push([command, ...args]);
    // The environment each command was given. `symbolicatecrash` cannot run without
    // DEVELOPER_DIR, so what the harness hands it is part of what these tests must pin.
    envs.push({ command, env: options?.env ?? null });
    if (command === "symbolicatecrash-path") {
      // Distinguish the negative run by the archive it was handed.
      const dsym = args[1];
      return { status: 0, stdout: dsym.includes("mismatch") ? negative : positive, stderr: "" };
    }
    if (command === "/usr/bin/python3") {
      // CrashSymbolicator writes its JSON to `-o`, not stdout, so the fixture does the same: a
      // runner that answered on stdout would let a broken harness read nothing and still pass.
      const dsym = args[args.indexOf("-d") + 1];
      const out = args[args.indexOf("-o") + 1];
      writeFileSync(out, String(dsym).includes("mismatch") ? negative : positive);
      return { status: 0, stdout: "", stderr: "" };
    }
    if (command === "xcrun" && args[0] === "clang") return { status: 0, stdout: "", stderr: "" };
    if (command === "xcrun" && args[0] === "dsymutil") {
      mkdirSync(join(args[args.indexOf("-o") + 1], "Contents", "Resources", "DWARF"), { recursive: true });
      return { status: 0, stdout: "", stderr: "" };
    }
    if (command === "dwarfdump") {
      const target = String(args.at(-1) ?? "");
      // The binary and its real archive carry ARM64; the throwaway fixture carries another UUID.
      const uuid = target.includes("mismatch") ? "4A1B2C3D-0000-1111-2222-333344445555" : ARM64;
      if (args[0] === "--statistics") {
        return { status: 0, stdout: JSON.stringify({ file: `${target} (arm64)`, "#functions": 1200 }), stderr: "" };
      }
      return { status: 0, stdout: `UUID: ${uuid} (arm64) ${target}\n`, stderr: "" };
    }
    // The collector itself, invoked as `<binary> daemon crash-report ...`.
    return { status: 0, stdout: JSON.stringify(collector), stderr: "" };
  };
}

function collectFixture(root) {
  const binaryBytes = macho();
  const crashPath = join(root, "ferryx-2026-11-16.ips");
  writeFileSync(crashPath, reportForPid({ pid: 4242, procPath: join(root, "ferryx") }));
  const dsymArchive = join(root, "dsyms");
  mkdirSync(join(dsymArchive, "ferryx.dSYM"), { recursive: true });
  const binary = join(root, "ferryx");
  writeFileSync(binary, binaryBytes);
  // The shipped tarball holding a byte-identical executable, so the identity check can pass.
  const artifact = join(root, "Ferryx.app.tar.gz");
  writeFileSync(artifact, tarGz([{ name: `./${APP_BINARY_MEMBER}`, bytes: binaryBytes }]));
  return { crashPath, dsymArchive, binary, artifact, binaryBytes };
}

/**
 * A forced-crash record shaped exactly like the one `acquireForcedCrash` builds.
 *
 * `spawnedSha256` and `sha256UnchangedAfterCrash` are part of that shape, not optional extras: the
 * release gate requires `binary.sha256` to be the digest captured before the crash, and the only
 * record of it is the forced crash. A fixture without them models a run the gate must refuse.
 */
function forcedRecordFor({ binary, pid = 4242, reportPath = "/tmp/reports/ferryx.ips", sha256 = undefined, unchanged = true } = {}) {
  // `acquireForcedCrash` always records the digest it read before sending the signal, so a fixture
  // of that record carries one too. Passing `sha256: null` models the run the gate must refuse.
  const digest =
    sha256 !== undefined
      ? sha256
      : (() => {
          try {
            return sha256Hex(readFileSync(binary));
          } catch {
            return null;
          }
        })();
  return {
    way: "forced-segv",
    pid,
    signal: "SIGSEGV",
    spawnedBinary: binary,
    spawnedSha256: digest,
    sha256UnchangedAfterCrash: unchanged,
    binaryUuids: [ARM64],
    // The two instants and the report's own header time. The provenance check orders the report
    // against the spawn, so a record without them is refused for the missing field rather than
    // judged on anything -- which is what made these fixtures stop proving the binding.
    spawnedAt: SPAWNED_AT,
    readyToken: "FERRYX_DAEMON_READY",
    readyAt: "2026-11-16T04:00:00.000Z",
    reportedAt: "2026-11-16T04:00:01.000Z",
    reportPath,
    reportPids: [pid],
    reportProcPaths: [binary],
    reportUuids: [ARM64],
    reportTimestamp: REPORT_TIMESTAMP,
    reportWrittenAt: REPORT_WRITTEN_AT,
    reportWrittenAfterCrash: true,
  };
}

test("the judge sequence collects, symbolizes, refuses the mismatch, and writes the report", () => withTmp((root) => {
  const { crashPath, dsymArchive, binary, artifact, binaryBytes } = collectFixture(root);
  const reportDir = join(root, "report");
  mkdirSync(reportDir);
  const dataDir = join(root, "data");
  mkdirSync(dataDir, { recursive: true });
  const daemonLog = join(dataDir, "daemon.log");
  writeFileSync(daemonLog, "daemon_panic thread=main at=pty.rs:412 payload=boom\n");
  const env = { FERRYX_DATA_DIR: dataDir };
  const calls = [];
  const envs = [];
  const developerDir = "/Applications/Xcode.app/Contents/Developer";
  const report = runSymbolicationChecks({
    binary,
    gate: fakeGate(),
    crashPath,
    dsymArchive,
    reportDir,
    packaged: artifact,
    tool: "symbolicatecrash-path",
    developerDir,
    env,
    forced: forcedRecordFor({ binary, sha256: sha256Hex(binaryBytes) }),
    run: judgeRun({
      collector: { reportDir: join(reportDir, "bundle"), crashReport: join(reportDir, "bundle", "ferryx-2026-11-16.ips"), daemonLog, dsym: join(dsymArchive, "ferryx.dSYM") },
      positive: NAMED_REPORT,
      negative: ANONYMOUS_REPORT,
      calls,
      envs,
    }),
  });

  assert.equal(report.verdict, "PASS");
  assert.equal(report.forcedCrash.pid, 4242);
  assert.equal(report.forcedCrash.signal, "SIGSEGV");
  assert.equal(report.forcedCrash.spawnedBinary, binary);
  // `binary.sha256` is required by the release gate; it must be the pre-crash digest, and the
  // record must show the bytes had not moved by the time the crash report was read.
  assert.equal(report.binary.sha256, sha256Hex(binaryBytes));
  assert.equal(report.binary.sha256, report.forcedCrash.spawnedSha256);
  assert.equal(report.binary.capturedBeforeCrash, true);
  assert.equal(report.binary.verifiedUnchangedAfterCrash, true);
  assert.deepEqual(report.binary.uuids, [ARM64]);
  // The exact bundles handed to the tool are recorded, so the run can be replayed by hand.
  assert.deepEqual(report.dsymTargets.paths, ["/tmp/dsyms/ferryx.dSYM"]);
  assert.deepEqual(report.packagedBinary.uuids, [ARM64]);
  assert.equal(report.packagedBinary.sha256, sha256Hex(readFileSync(binary)));
  assert.equal(report.collector.daemonLogFromCrashedRoot, true);
  assert.ok(report.collector.daemonLogBytes > 0);
  assert.equal(report.negative.gateCode, "DSYM_UUID_MISMATCH");
  assert.equal(report.negative.namedFrames, 0);
  assert.equal(report.symbolicated.namedFrames, 2);
  // Every named check the release gate reads must be present and true.
  for (const name of [
    "crashReportNamesBinary",
    "namedFramePresent",
    "gateRefusedMismatch",
    "mismatchNamesNothing",
    "collectorCopiedCrashReport",
    "forcedCrashProvenance",
    "binaryIdentity",
    "packagedBinaryIdentity",
  ]) {
    assert.ok(report.checks[name], `missing check ${name}`);
    assert.equal(report.checks[name].ok, true, `${name} must pass`);
  }
  // The collector ran with the crash's environment, not the harness's own.
  const collectorCall = calls.find((call) => call[1] === "daemon");
  assert.deepEqual(collectorCall.slice(0, 6), [binary, "daemon", "crash-report", "--ips", crashPath, "--dsym"]);
  // The negative run really used a different archive, or it would prove nothing.
  assert.ok(calls.some((call) => call[0] === "symbolicatecrash-path" && String(call[2]).includes("mismatch")));
  // The positive run was handed the resolved bundle path, not a directory to search.
  const positiveCall = calls.find((call) => call[0] === "symbolicatecrash-path" && !String(call[2]).includes("mismatch"));
  assert.equal(positiveCall[2], "/tmp/dsyms/ferryx.dSYM");
  // Both runs carry the DEVELOPER_DIR the tool refuses to start without, and the report records
  // both the path and the environment so the command can be replayed by hand.
  for (const call of envs.filter((entry) => entry.command === "symbolicatecrash-path")) {
    assert.equal(call.env.DEVELOPER_DIR, developerDir);
  }
  assert.equal(report.symbolicatecrash, "symbolicatecrash-path");
  assert.deepEqual(report.symbolicatecrashEnv, { developerDir, env: { DEVELOPER_DIR: developerDir } });
  assert.deepEqual(report.redactedProcPaths, []);

  // The evidence files are on disk, so an operator can read what was observed.
  const written = JSON.parse(readFileSync(join(reportDir, "report.json"), "utf8"));
  assert.equal(written.verdict, "PASS");
  // The persisted report is what the release gate reads, so `binary.sha256` must survive the write.
  assert.equal(written.binary.sha256, sha256Hex(binaryBytes));
  assert.match(readFileSync(join(reportDir, "positive-symbolicated.txt"), "utf8"), /PtySession::read_frame/);
  assert.ok(existsSync(join(reportDir, "crash-report-path.txt")));
}));

/**
 * The measured failure: the crash report named `/Users/USER/<*>/Ferryx.app/Contents/MacOS/ferryx`
 * while this run spawned the same executable under the real home, and the exact comparison failed.
 *
 * The judge must accept that redaction (it is macOS's, not the fixture's), state in the detail that
 * the path was matched in redacted form, keep the report's own path as the OS wrote it, and still
 * refuse a redaction whose tail differs.
 */
test("the judge accepts a home-redacted report path and says so", () => withTmp((root) => {
  const { crashPath, dsymArchive, binary, artifact } = collectFixture(root);
  // A home under the temp root, so the fixture never writes into a real `/Users/<name>`.
  const home = join(root, "home");
  // The binary sits under that home, the way a release run directory does.
  const underHome = join(home, "code", "ferryx-wt", ".evidence", "phase0", "bin", "pkg", APP_BINARY_MEMBER);
  mkdirSync(dirname(underHome), { recursive: true });
  writeFileSync(underHome, readFileSync(binary));
  const redacted = `${REDACTED_HOME_PREFIX}*/${APP_BINARY_MEMBER}`;

  const runCase = (reportProcPaths) => {
    const reportDir = join(root, `report-${reportProcPaths.join("-").replace(/[^a-z0-9]+/gi, "_")}`);
    mkdirSync(reportDir);
    return runSymbolicationChecks({
      binary: underHome,
      gate: fakeGate(),
      crashPath,
      dsymArchive,
      reportDir,
      packaged: artifact,
      tool: "symbolicatecrash-path",
      env: null,
      home,
      forced: { ...forcedRecordFor({ binary: underHome }), reportProcPaths },
      run: judgeRun({
        // The collector's own report, so the run reaches the judgements instead of dying on an
        // unreadable bundle: the clause under test is the provenance path, not the bundle.
        collector: {
          reportDir: join(reportDir, "bundle"),
          crashReport: join(reportDir, "bundle", basename(crashPath)),
          dsym: join(dsymArchive, "ferryx.dSYM"),
        },
        positive: NAMED_REPORT,
        negative: ANONYMOUS_REPORT,
      }),
    });
  };

  // Redacted and consistent: past the path clause, with the substitution stated.
  const accepted = runCase([redacted]);
  assert.equal(accepted.checks.forcedCrashProvenance.ok, true, accepted.checks.forcedCrashProvenance.detail);
  assert.match(accepted.checks.forcedCrashProvenance.detail, /matched in redacted form/);
  assert.deepEqual(accepted.redactedProcPaths.map((match) => match.reported), [redacted]);
  // The evidence still carries what the OS wrote, not the rewritten path.
  assert.deepEqual(accepted.forcedCrash.reportProcPaths, [redacted]);

  // A redaction naming a different archive is not this run's binary, so the check fails.
  const wrongTail = runCase([`${REDACTED_HOME_PREFIX}*/Other.app/Contents/MacOS/ferryx`]);
  assert.equal(wrongTail.checks.forcedCrashProvenance.ok, false);
  assert.match(wrongTail.checks.forcedCrashProvenance.detail, /not the binary this run spawned/);
  assert.deepEqual(wrongTail.redactedProcPaths, []);
}));

test("a log that did not come from the crashed root is not accepted as its diagnostics", () => withTmp((root) => {
  const { crashPath, dsymArchive, binary, artifact } = collectFixture(root);
  const reportDir = join(root, "report");
  mkdirSync(reportDir);
  const dataDir = join(root, "data");
  mkdirSync(dataDir, { recursive: true });
  // Some other process's log, outside the isolated root this crash ran under.
  const foreignLog = join(root, "elsewhere", "daemon.log");
  mkdirSync(join(root, "elsewhere"), { recursive: true });
  writeFileSync(foreignLog, "unrelated\n");
  const report = runSymbolicationChecks({
    binary,
    gate: fakeGate(),
    crashPath,
    dsymArchive,
    reportDir,
    packaged: artifact,
    tool: "symbolicatecrash-path",
    env: { FERRYX_DATA_DIR: dataDir },
    forced: forcedRecordFor({ binary }),
    run: judgeRun({
      collector: { crashReport: join(reportDir, "bundle", "ferryx-2026-11-16.ips"), daemonLog: foreignLog },
      positive: NAMED_REPORT,
      negative: ANONYMOUS_REPORT,
    }),
  });
  assert.equal(report.collector.daemonLogFromCrashedRoot, false);
  assert.equal(report.verdict, "PASS", "the log location is recorded, not silently swapped for a pass");
}));

/**
 * A release run directory laid out the way `release-local` builds one: the artifact the updater
 * ships at `artifacts/darwin/Ferryx.app.tar.gz`, the dSYM archive beside it with its
 * self-describing manifest, and the symbolication evidence where the gate looks for it.
 */
function releaseLayout(root) {
  const fixture = collectFixture(root);
  const releaseDir = join(root, "run");
  const artifact = join(releaseDir, "artifacts", "darwin", "Ferryx.app.tar.gz");
  mkdirSync(dirname(artifact), { recursive: true });
  writeFileSync(artifact, tarGz([{ name: APP_BINARY_MEMBER, bytes: fixture.binaryBytes }]));
  const dsymDir = join(releaseDir, "dsyms");
  writeDsymArchiveManifest(
    dsymDir,
    buildDsymArchiveManifest({
      runDir: releaseDir,
      binaries: [{ binary: fixture.binary, uuids: [ARM64], functions: { arm64: 1200 }, dsym: join(dsymDir, "ferryx.dSYM") }],
      gates: [{ binaryPath: fixture.binary, binaryUuids: [ARM64], bundles: [{ path: join(dsymDir, "ferryx.dSYM") }] }],
    }),
  );
  const reportDir = join(releaseDir, "symbolication");
  mkdirSync(reportDir, { recursive: true });
  return { ...fixture, releaseDir, artifact, dsymDir, reportDir };
}

/**
 * The seam between the two owners: the report this harness writes must satisfy the release gate as
 * it actually ships, with `binary.sha256` REQUIRED.
 *
 * Asserting the report's internal shape is not enough. `assertSymbolicationGate` is the code that
 * decides whether a release may be drafted, and it compares `report.binary.sha256` with the member
 * inside the packaged tarball -- a field the harness has to supply and the gate refuses to infer.
 * This runs the real gate against a real report and real bytes, so the contract is proven rather
 * than assumed, without touching the gate's own file.
 */
test("the evidence report satisfies the release gate's required binary.sha256", () => withTmp((root) => {
  const { crashPath, dsymArchive, binary, artifact, binaryBytes, releaseDir, reportDir } = releaseLayout(root);
  const report = runSymbolicationChecks({
    binary,
    gate: fakeGate(),
    crashPath,
    dsymArchive,
    reportDir,
    packaged: artifact,
    tool: "symbolicatecrash-path",
    forced: forcedRecordFor({ binary }),
    // The bundle must name the report this crash produced, or the acceptance itself fails and the
    // gate would be refusing for the wrong reason.
    run: judgeRun({ collector: { crashReport: crashPath }, positive: NAMED_REPORT, negative: ANONYMOUS_REPORT }),
  });
  assert.equal(report.verdict, "PASS");
  // The digest the gate compares is the pre-crash digest, and it is the tarball member's too.
  assert.equal(report.binary.sha256, sha256Hex(binaryBytes));
  assert.equal(report.packagedBinary.sha256, sha256Hex(binaryBytes));

  const { result } = assertSymbolicationGate({ releaseDir });
  assert.equal(result.ok, true, JSON.stringify(result.blocking));
  assert.deepEqual(result.archived, [ARM64]);
  assert.equal(result.packagedSha256, sha256Hex(binaryBytes));
  // The gate's own view of the forced crash came from the report, not from a default.
  assert.equal(result.forcedCrash.signal, "SIGSEGV");
  assert.equal(result.forcedCrash.pid, 4242);
}));

test("the release gate refuses the evidence when binary.sha256 is absent", () => withTmp((root) => {
  const { crashPath, dsymArchive, binary, artifact, releaseDir, reportDir } = releaseLayout(root);
  const report = runSymbolicationChecks({
    binary,
    gate: fakeGate(),
    crashPath,
    dsymArchive,
    reportDir,
    packaged: artifact,
    tool: "symbolicatecrash-path",
    forced: forcedRecordFor({ binary }),
    run: judgeRun({ collector: { crashReport: crashPath }, positive: NAMED_REPORT, negative: ANONYMOUS_REPORT }),
  });
  // The acceptance itself must be a clean PASS first: a run that already failed would make the
  // refusal below prove nothing about the digest field.
  assert.equal(report.verdict, "PASS", JSON.stringify(report.checks));
  // Remove exactly the field the gate added to its contract: nothing else about the evidence
  // changes, so a refusal here can only be about the missing digest.
  const reportPath = join(reportDir, "report.json");
  const persisted = JSON.parse(readFileSync(reportPath, "utf8"));
  delete persisted.binary.sha256;
  writeFileSync(reportPath, JSON.stringify(persisted));

  const { blocking } = evaluateSymbolicationEvidence({ releaseDir });
  const finding = blocking.find((problem) => problem.code === "SYMBOLICATION_BINARY_UNHASHED");
  assert.ok(finding, `expected SYMBOLICATION_BINARY_UNHASHED, got ${blocking.map((p) => p.code).join(", ")}`);
}));

test("the release gate refuses evidence for bytes the tarball does not ship", () => withTmp((root) => {
  const { crashPath, dsymArchive, binary, artifact, binaryBytes, releaseDir, reportDir } = releaseLayout(root);
  runSymbolicationChecks({
    binary,
    gate: fakeGate(),
    crashPath,
    dsymArchive,
    reportDir,
    packaged: artifact,
    tool: "symbolicatecrash-path",
    forced: forcedRecordFor({ binary }),
    run: judgeRun({ collector: { crashReport: crashPath }, positive: NAMED_REPORT, negative: ANONYMOUS_REPORT }),
  });
  // A tarball carrying different bytes under the same member path: the UUIDs and every check still
  // agree, so only the digest comparison can catch that the symbolicated bytes never shipped.
  writeFileSync(artifact, tarGz([{ name: APP_BINARY_MEMBER, bytes: Buffer.concat([binaryBytes, Buffer.from("patched")]) }]));

  const { blocking } = evaluateSymbolicationEvidence({ releaseDir });
  const finding = blocking.find((problem) => problem.code === "SYMBOLICATION_BINARY_NOT_SHIPPED");
  assert.ok(finding, `expected SYMBOLICATION_BINARY_NOT_SHIPPED, got ${blocking.map((p) => p.code).join(", ")}`);
  assert.match(finding.detail, /different bytes/);
}));

test("a bundle holding a different crash report fails the acceptance", () => withTmp((root) => {
  const { crashPath, dsymArchive, binary, artifact } = collectFixture(root);
  const reportDir = join(root, "report");
  mkdirSync(reportDir);
  const report = runSymbolicationChecks({
    binary,
    gate: fakeGate(),
    crashPath,
    dsymArchive,
    reportDir,
    packaged: artifact,
    tool: "symbolicatecrash-path",
    forced: forcedRecordFor({ binary }),
    run: judgeRun({
      // The collector resolved some other .ips: the bundle does not describe this crash.
      collector: { crashReport: join(reportDir, "bundle", "some-older.ips") },
      positive: NAMED_REPORT,
      negative: ANONYMOUS_REPORT,
    }),
  });
  assert.equal(report.checks.collectorCopiedCrashReport.ok, false);
  assert.equal(report.verdict, "FAIL");
}));

test("a collector that names no copied report fails the acceptance", () => withTmp((root) => {
  const { crashPath, dsymArchive, binary, artifact } = collectFixture(root);
  const reportDir = join(root, "report");
  mkdirSync(reportDir);
  const report = runSymbolicationChecks({
    binary,
    gate: fakeGate(),
    crashPath,
    dsymArchive,
    reportDir,
    packaged: artifact,
    tool: "symbolicatecrash-path",
    forced: forcedRecordFor({ binary }),
    run: judgeRun({ collector: {}, positive: NAMED_REPORT, negative: ANONYMOUS_REPORT }),
  });
  assert.equal(report.checks.collectorCopiedCrashReport.ok, false);
  assert.equal(report.verdict, "FAIL");
}));

/**
 * A stand-in child whose stdout/exit this test drives by hand.
 *
 * stdout is a plain EventEmitter, not a stream, so `emit("data")` runs the handler synchronously
 * and the "never signalled before readiness" assertion orders deterministically against the write
 * that follows it.
 */
function fakeChild() {
  const child = new EventEmitter();
  child.pid = 4242;
  child.stdout = Object.assign(new EventEmitter(), { write: (text) => child.stdout.emit("data", Buffer.from(text)) });
  child.stderr = Object.assign(new EventEmitter(), { write: (text) => child.stderr.emit("data", Buffer.from(text)) });
  child.signals = [];
  child.kill = (signal) => {
    child.signals.push(signal);
    return true;
  };
  return child;
}

test("a forced crash signals ONLY after readiness, and only SIGSEGV", () => {
  const child = fakeChild();
  const pending = forceSegv({
    binary: "/tmp/ferryx",
    env: {},
    readyDeadlineMs: 5_000,
    exitDeadlineMs: 5_000,
    spawnImpl: (command, args, options) => {
      assert.equal(command, "/tmp/ferryx");
      assert.deepEqual(args, ["--daemon"]);
      assert.equal(options.stdio[0], "ignore");
      return child;
    },
  });
  // Before readiness the daemon is never signalled: killing a booting daemon proves no crash site.
  child.stdout.write("booting\n");
  assert.deepEqual(child.signals, []);
  child.stdout.write("FERRYX_DAEMON_READY\n");
  assert.deepEqual(child.signals, ["SIGSEGV"]);
  child.emit("exit", null, "SIGSEGV");
  return pending.then((outcome) => {
    assert.equal(outcome.pid, 4242);
    assert.equal(outcome.signal, "SIGSEGV");
    // One delivery only, and it is recorded: a process that dies of the first signal must not be
    // signalled a second time.
    assert.deepEqual(child.signals, ["SIGSEGV"]);
    assert.equal(outcome.signalCount, 1);
    assert.deepEqual(
      outcome.signalDeliveries.map((delivery) => ({ attempt: delivery.attempt, pid: delivery.pid, signal: delivery.signal })),
      [{ attempt: 1, pid: 4242, signal: "SIGSEGV" }],
    );
    assert.ok(Number.isFinite(Date.parse(outcome.signalDeliveries[0].at)));
  });
});

/**
 * The measured failure this fixes: the Rust runtime installs a SIGSEGV handler for stack-overflow
 * detection and, for a `kill(2)`-delivered signal, restores SIG_DFL and returns instead of dying.
 * The daemon therefore survived the first SIGSEGV and was SIGKILLed by the exit deadline, which the
 * harness reported as "left with SIGKILL instead of SIGSEGV".
 */
test("a swallowed SIGSEGV is re-delivered once to the SAME pid, and the exit is still SIGSEGV", () => {
  const child = fakeChild();
  // The first signal is swallowed (SIG_DFL is restored); the second terminates the process.
  child.kill = (signal) => {
    child.signals.push(signal);
    if (child.signals.length === 2) queueMicrotask(() => child.emit("exit", null, "SIGSEGV"));
    return true;
  };
  const pending = forceSegv({
    binary: "/tmp/ferryx",
    env: {},
    readyDeadlineMs: 5_000,
    exitDeadlineMs: 5_000,
    segvRetryMs: 5,
    spawnImpl: () => child,
  });
  child.stdout.write("FERRYX_DAEMON_READY\n");
  assert.deepEqual(child.signals, ["SIGSEGV"]);
  return pending.then((outcome) => {
    assert.deepEqual(child.signals, ["SIGSEGV", "SIGSEGV"]);
    // The outcome is the crash the acceptance needs, not a SIGKILL: the retry is how the signal is
    // made to land, not a different ending.
    assert.equal(outcome.signal, "SIGSEGV");
    assert.equal(outcome.signalCount, 2);
    assert.deepEqual(
      outcome.signalDeliveries.map((delivery) => ({ attempt: delivery.attempt, pid: delivery.pid, signal: delivery.signal })),
      [
        { attempt: 1, pid: 4242, signal: "SIGSEGV" },
        { attempt: 2, pid: 4242, signal: "SIGSEGV" },
      ],
    );
    // Both deliveries carry a timestamp, so the report can show the host needed two.
    for (const delivery of outcome.signalDeliveries) assert.ok(Number.isFinite(Date.parse(delivery.at)));
  });
});

test("a SIGSEGV that ends the process synchronously never gets a second signal", () => {
  const child = fakeChild();
  // The child dies inside the `kill` call itself: the re-delivery must not be armed at all, and a
  // pid that is already gone must never be signalled again.
  child.kill = (signal) => {
    child.signals.push(signal);
    child.emit("exit", null, "SIGSEGV");
    return true;
  };
  const pending = forceSegv({ binary: "/tmp/ferryx", env: {}, segvRetryMs: 0, spawnImpl: () => child });
  child.stdout.write("FERRYX_DAEMON_READY\n");
  return pending.then((outcome) => {
    assert.deepEqual(child.signals, ["SIGSEGV"]);
    assert.equal(outcome.signal, "SIGSEGV");
    assert.equal(outcome.signalCount, 1);
  });
});

test("a clean exit is refused, not scored as a crash", () => {
  const child = fakeChild();
  const pending = forceSegv({
    binary: "/tmp/ferryx",
    env: {},
    spawnImpl: () => child,
  });
  child.stdout.write("FERRYX_DAEMON_READY\n");
  child.emit("exit", 0, null);
  return pending.then(
    () => assert.fail("a zero exit must not resolve the forced crash"),
    (error) => {
      assert.equal(error.code, "UNEXPECTED_EXIT");
      assert.equal(error.exit, 1);
    },
  );
});

test("a daemon that dies during boot is reported at once, not after the readiness window", () => {
  const child = fakeChild();
  const pending = forceSegv({ binary: "/tmp/ferryx", env: {}, readyDeadlineMs: 60_000, spawnImpl: () => child });
  child.stderr.write("dyld: Library not loaded\n");
  child.emit("exit", 1, null);
  return pending.then(
    () => assert.fail("a boot-time exit must not resolve the forced crash"),
    (error) => {
      assert.equal(error.code, "DAEMON_EXITED_BEFORE_READY");
      assert.equal(error.exit, 3);
      assert.match(error.detail.stderr, /Library not loaded/);
    },
  );
});

test("a daemon that never announces readiness is an environment gap", () => {
  const child = fakeChild();
  return forceSegv({ binary: "/tmp/ferryx", env: {}, readyDeadlineMs: 20, spawnImpl: () => child }).then(
    () => assert.fail("no readiness must not resolve"),
    (error) => {
      assert.equal(error.code, "DAEMON_NOT_READY");
      assert.equal(error.exit, 3);
    },
  );
});

/** The measured host: Xcode ships CrashSymbolicator.py beside the frameworks that hold it. */
const XCODE_DEVELOPER_DIR = "/Applications/Xcode.app/Contents/Developer";
const CRASH_SYMBOLICATOR = "/Applications/Xcode.app/Contents/SharedFrameworks/CoreSymbolicationDT.framework/Versions/A/Resources/CrashSymbolicator.py";

/** A modern `.ips`: a one-line JSON header, then the body ReportCrash wrote. */
function ipsReport(body) {
  return `${JSON.stringify({ app_name: "ferryx", bug_type: "309", timestamp: REPORT_TIMESTAMP })}\n${JSON.stringify(body)}`;
}

/**
 * What CrashSymbolicator writes with `--no-ips-header`: the same body, with `symbol` filled in.
 *
 * The shape is the measured output of the real tool, not an invention: `threads[].frames[]` entries
 * keep `imageOffset`/`imageIndex` and gain `symbol`/`symbolLocation`, and `usedImages[]` keeps the
 * `uuid` that decides which dSYM names which frame.
 */
function symbolizedIps({ frames, images }) {
  return JSON.stringify({ threads: [{ triggered: true, frames }], usedImages: images });
}

const KERNEL_IMAGE = { name: "libsystem_kernel.dylib", uuid: "11111111-1111-1111-1111-111111111111", base: 0x18f000000 };

/**
 * The measured failure this fixes: `symbolicatecrash` exited 255 with
 * `No crash report version in .../ferryx-...ips` because it cannot read a JSON crash log at all.
 *
 * Xcode ships the replacement, so a JSON report must be routed there with the exact argv the tool
 * documents -- `-d <dsym>`, `--no-ips-header`, `-o <file>` -- and with the `DEVELOPER_DIR` the
 * harness already resolved. `/usr/bin/python3` is pinned because the tool imports Xcode's own
 * modules, which an unrelated interpreter does not resolve.
 */
test("a modern .ips is symbolicated by CrashSymbolicator.py, with the argv and env it needs", () => withTmp((root) => {
  const ips = "/reports/ferryx-2026-11-19-195346.ips";
  const dsym = "/tmp/dsyms/ferryx.dSYM";
  const desymbolicatedDir = join(root, "desymbolicated");
  const report = ipsReport({
    usedImages: [KERNEL_IMAGE, { name: "ferryx", uuid: ARM64, base: 0x100000000 }],
    threads: [{ frames: [{ imageOffset: 1234, imageIndex: 1 }] }],
  });
  const symbolicated = symbolizedIps({
    images: [KERNEL_IMAGE, { name: "ferryx", uuid: ARM64, base: 0x100000000 }],
    frames: [{ imageOffset: 1234, symbol: "parking_lot_core::parking_lot::park::park", symbolLocation: 20, imageIndex: 1 }],
  });
  const calls = [];
  const written = [];
  const result = symbolicate({
    tool: "/Applications/Xcode.app/Contents/SharedFrameworks/DVTFoundation.framework/Versions/A/Resources/symbolicatecrash",
    crashSymbolicator: CRASH_SYMBOLICATOR,
    ips,
    dsym,
    env: symbolicationEnv({ developerDir: XCODE_DEVELOPER_DIR, base: { PATH: "/usr/bin" } }),
    binaryUuids: [ARM64],
    run: (command, args, options) => {
      calls.push({ command, args, env: options?.env });
      return { status: 0, stdout: "", stderr: "" };
    },
    readText: (file) => (file === ips ? report : symbolicated),
    makeOutDir: () => "/tmp/ferryx-crash-symbolicator-fixture",
    removeDir: () => {},
    // The copy is captured instead of written: this test pins what the tool is HANDED, and a real
    // write would make it depend on a directory outside the fixture.
    desymbolicatedDir,
    writeText: (file, data) => written.push({ file, data }),
  });

  // ONE call, to python3, and it is the JSON symbolicator -- not symbolicatecrash, which cannot read
  // this report and would return 255 with zero frames.
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, "/usr/bin/python3");
  assert.equal(result.symbolicator, CRASH_SYMBOLICATOR_TOOL);
  assert.deepEqual(calls[0].args, [
    CRASH_SYMBOLICATOR,
    // ONE `-d`: the flag is argparse's `store`, so a repeated one would replace rather than add.
    "-d",
    dsym,
    "--no-ips-header",
    "-o",
    join("/tmp/ferryx-crash-symbolicator-fixture", "symbolicated.json"),
    // The tool is handed the DE-SYMBOLICATED copy, never the raw report: ReportCrash names frames
    // itself from a UUID-keyed cache that survives `strip`, so a raw report can arrive fully named.
    join(desymbolicatedDir, DESYMBOLICATED_REPORT_NAME),
  ]);
  assert.equal(result.desymbolicatedReport, join(desymbolicatedDir, DESYMBOLICATED_REPORT_NAME));
  assert.equal(result.preNamedFrames, 0);
  assert.equal(written.length, 1);
  assert.equal(written[0].file, result.desymbolicatedReport);
  // The copy is still an `.ips`: the header line comes through untouched.
  assert.equal(written[0].data.split("\n")[0], report.split("\n")[0]);
  assert.equal(calls[0].env.DEVELOPER_DIR, XCODE_DEVELOPER_DIR);
  assert.equal(calls[0].env.PATH, "/usr/bin");
  assert.equal(result.status, 0);
  assert.equal(result.jsonReport, true);
  // Nothing was read from stdout by accident: CrashSymbolicator writes its result to `-o`.
  assert.equal(result.stdout, symbolicated);
  // The frames are handed on in the same column form the text tool's output is read in.
  assert.match(result.framesText, /ferryx\s+0x1000004d2\s+parking_lot_core::parking_lot::park::park \+ 20/);
}));

/**
 * The naming rule for a JSON report: a ferryx frame is the one whose `usedImages[imageIndex]` UUID is
 * a build UUID of the accepted binary, or whose image is named `ferryx`.
 *
 * The UUID clause is what the dSYM gate is keyed to, so a frame of a SYSTEM image that happens to
 * carry a symbol proves nothing about the archive and must not be counted.
 */
test("a JSON report's named ferryx frame is counted through imageIndex -> usedImages uuid", () => {
  const images = [
    KERNEL_IMAGE,
    // No `name` at all, and an undashed lowercase UUID: the mapping must come from the UUID.
    { path: "/tmp/pkg/renamed-executable", uuid: ARM64.replace(/-/g, "").toLowerCase(), base: 0x100000000 },
  ];
  const text = symbolizedIps({
    images,
    frames: [
      // A named SYSTEM frame: proves the binary filter runs before the named check.
      { imageOffset: 100, symbol: "__pthread_kill", symbolLocation: 8, imageIndex: 0 },
      // The binary's own frame, named.
      { imageOffset: 200, symbol: "ferryx::terminal::pty::PtySession::read_frame", symbolLocation: 32, imageIndex: 1 },
    ],
  });
  const rendered = renderJsonCrashFrames({ text, binaryUuids: [ARM64] });
  assert.ok(rendered);
  // The renamed image is labelled with the accepted binary's name, not its path's basename.
  assert.match(rendered.text, new RegExp(`${BINARY_FRAME_LABEL}\\s+0x1000000c8`));
  assert.doesNotMatch(rendered.text, /renamed-executable/);

  const verdict = evaluateSymbolication({ crashText: MODERN_REPORT, symbolicatedText: rendered.text });
  assert.equal(verdict.frames.length, 2);
  assert.equal(verdict.binaryFrames.length, 1, "the system frame is not a frame of the binary");
  assert.equal(verdict.namedFrames.length, 1);
  assert.equal(verdict.namedFrames[0].body, "ferryx::terminal::pty::PtySession::read_frame + 32");
  assert.equal(verdict.checks.namedFramePresent.ok, true);
});

/**
 * The state Phase 0 exists to end, in the JSON shape: a ferryx frame with no symbol stays anonymous.
 *
 * A frame that names nothing must fail the acceptance, so an offset-only frame is rendered as the
 * offset pair `classifyBody` already reads as [`ANONYMOUS`] -- exactly what the text tool prints.
 */
test("an unnamed ferryx frame in a JSON report is counted as anonymous, not named", () => {
  const images = [KERNEL_IMAGE, { name: "ferryx", uuid: ARM64, base: 0x100000000 }];
  const text = symbolizedIps({ images, frames: [{ imageOffset: 7401632, imageIndex: 1, symbolLocation: 0 }] });
  const rendered = renderJsonCrashFrames({ text, binaryUuids: [ARM64] });
  const verdict = evaluateSymbolication({ crashText: MODERN_REPORT, symbolicatedText: rendered.text });
  assert.equal(verdict.binaryFrames.length, 1);
  assert.equal(verdict.namedFrames.length, 0);
  assert.equal(verdict.anonymousFrames.length, 1);
  assert.equal(verdict.anonymousFrames[0].kind, ANONYMOUS);
  assert.equal(verdict.checks.namedFramePresent.ok, false);
});

/**
 * The legacy text path is untouched: a `.crash`-style report still goes to `symbolicatecrash`, whose
 * column output is read exactly as before. A JSON report on a host with no CrashSymbolicator.py is
 * also recorded as such, rather than silently scored as a tool that found nothing.
 */
test("a legacy text report is still symbolicated by symbolicatecrash", () => {
  const ips = "/reports/ferryx-legacy.crash";
  const legacy = ["Incident Identifier: 5F5E68AE", "Process: ferryx [4242]", "0   ferryx   0x104a3f2a0   ???"].join("\n");
  const calls = [];
  const run = (command, args, options) => {
    calls.push({ command, args, env: options?.env });
    return { status: 0, stdout: NAMED_REPORT, stderr: "" };
  };

  const text = symbolicate({
    tool: "/usr/bin/symbolicatecrash",
    crashSymbolicator: CRASH_SYMBOLICATOR,
    ips,
    dsym: "/tmp/dsyms/ferryx.dSYM",
    env: { DEVELOPER_DIR: XCODE_DEVELOPER_DIR },
    run,
    readText: () => legacy,
  });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, "/usr/bin/symbolicatecrash");
  assert.deepEqual(calls[0].args, [ips, "/tmp/dsyms/ferryx.dSYM"]);
  assert.equal(text.symbolicator, "symbolicatecrash");
  assert.equal(text.jsonReport, false);
  assert.equal(text.framesText, NAMED_REPORT);
  // The shape decides the route, and both real forms are recognised: an `.ips` header naming
  // `bug_type`, and a bare body carrying `threads`/`usedImages` (what `--no-ips-header` writes).
  assert.equal(isJsonCrashReport(legacy), false);
  assert.equal(isJsonCrashReport(MODERN_REPORT), true);
  assert.equal(isJsonCrashReport(symbolizedIps({ images: [], frames: [] })), true);

  // A JSON report with no CrashSymbolicator.py is a host gap the report states: symbolicatecrash is
  // asked anyway (and exits 255 on this shape), and `jsonReport` records why that is not a verdict.
  const fallback = symbolicate({
    tool: "/usr/bin/symbolicatecrash",
    crashSymbolicator: null,
    ips: "/reports/ferryx.ips",
    dsym: "/tmp/dsyms/ferryx.dSYM",
    run: (command, args) => {
      calls.push({ command, args });
      return { status: 255, stdout: "", stderr: "No crash report version in /reports/ferryx.ips\n" };
    },
    readText: () => MODERN_REPORT,
  });
  assert.equal(fallback.symbolicator, "symbolicatecrash");
  assert.equal(fallback.jsonReport, true);
  assert.equal(fallback.status, 255);
});

/**
 * Discovery of the JSON tool: the env override, the measured Xcode path, and absence.
 *
 * `resolveSymbolicatecrash` reports both tools from ONE developer dir, so a report states which
 * symbolicator the host could offer as well as the one that ran.
 */
test("CrashSymbolicator.py is discovered beside the frameworks, with an env override", () => {
  assert.equal(
    resolveCrashSymbolicator({ env: { CRASH_SYMBOLICATOR: "/opt/cs/CrashSymbolicator.py" }, developerDir: XCODE_DEVELOPER_DIR }),
    "/opt/cs/CrashSymbolicator.py",
  );
  assert.equal(
    resolveCrashSymbolicator({ env: {}, developerDir: XCODE_DEVELOPER_DIR, exists: (candidate) => candidate === CRASH_SYMBOLICATOR }),
    CRASH_SYMBOLICATOR,
  );
  // The path is derived from the developer dir, never from PATH.
  assert.equal(
    resolveCrashSymbolicator({ env: {}, developerDir: XCODE_DEVELOPER_DIR, exists: (candidate) => {
      assert.ok(candidate.startsWith("/Applications/Xcode.app/Contents/SharedFrameworks/"));
      return false;
    } }),
    null,
  );
  assert.equal(resolveCrashSymbolicator({ env: {}, developerDir: null }), null);

  // One lookup reports both tools, so the evidence can state that this host CAN read a JSON report.
  const resolved = resolveSymbolicatecrash({
    env: { DEVELOPER_DIR: XCODE_DEVELOPER_DIR },
    run: () => ({ status: 1, stdout: "", stderr: "" }),
    exists: (candidate) => candidate === CRASH_SYMBOLICATOR,
  });
  assert.equal(resolved.tool, null);
  assert.equal(resolved.developerDir, XCODE_DEVELOPER_DIR);
  assert.equal(resolved.crashSymbolicator, CRASH_SYMBOLICATOR);
});

/**
 * Both halves of the acceptance in ONE judge run through the JSON tool: the positive report names a
 * frame, and the SAME tool names nothing under the mismatched dSYM.
 *
 * This is the rule that must not be special-cased: CrashSymbolicator only opens a dSYM whose UUID
 * matches the image's, so a mismatched archive yields symbol-less frames on its own.
 */
test("the judge routes both runs through CrashSymbolicator.py", () => withTmp((root) => {
  const { crashPath, dsymArchive, binary, artifact } = collectFixture(root);
  const reportDir = join(root, "report");
  mkdirSync(reportDir);
  const images = [KERNEL_IMAGE, { name: "ferryx", uuid: ARM64, base: 0x100000000 }];
  // The report on disk is the shape a real host writes: frames of the binary that ReportCrash already
  // named. The copy the judge writes is what the two tool runs are handed.
  writeFileSync(
    crashPath,
    ipsReport({
      usedImages: images,
      threads: [{ frames: [{ imageOffset: 512, symbol: "ferryx::daemon::server::serve::h1", symbolLocation: 8, imageIndex: 1 }] }],
    }),
  );
  const named = symbolizedIps({ images, frames: [{ imageOffset: 512, symbol: "ferryx::daemon::server::serve", symbolLocation: 8, imageIndex: 1 }] });
  const unnamed = symbolizedIps({ images, frames: [{ imageOffset: 512, imageIndex: 1 }] });
  const calls = [];
  const envs = [];
  const report = runSymbolicationChecks({
    binary,
    gate: fakeGate(),
    crashPath,
    dsymArchive,
    reportDir,
    packaged: artifact,
    tool: "symbolicatecrash-path",
    crashSymbolicator: CRASH_SYMBOLICATOR,
    developerDir: XCODE_DEVELOPER_DIR,
    forced: forcedRecordFor({ binary }),
    run: judgeRun({ collector: { crashReport: crashPath }, positive: named, negative: unnamed, calls, envs }),
  });

  assert.equal(report.symbolicator, CRASH_SYMBOLICATOR_TOOL);
  assert.equal(report.symbolicated.symbolicator, CRASH_SYMBOLICATOR_TOOL);
  assert.equal(report.symbolicated.reportFormat, "ips-json");
  // The same tool ran twice: once for the archive under test, once for the mismatched fixture.
  const toolCalls = envs.filter((entry) => entry.command === "/usr/bin/python3");
  assert.equal(toolCalls.length, 2);
  for (const entry of toolCalls) assert.equal(entry.env.DEVELOPER_DIR, XCODE_DEVELOPER_DIR);
  const mismatchCall = calls.find((call) => call[0] === "/usr/bin/python3" && String(call[3]).includes("mismatch"));
  assert.ok(mismatchCall, "the negative run must go through CrashSymbolicator too");
  // The routing is what the acceptance depends on: a FAIL here would mean the JSON frames were not
  // counted, not that the archive failed to name them.
  assert.equal(report.verdict, "PASS", JSON.stringify(report.checks));
  // Each run's `-d` is the directory that run is about. The negative run must NOT be handed the
  // archive that just named frames, or it would be given the real dSYM and prove nothing at all.
  const positiveCall = calls.find((call) => call[0] === "/usr/bin/python3" && !String(call[3]).includes("mismatch"));
  assert.equal(positiveCall[3], dsymArchive);
  assert.ok(!String(mismatchCall[3]).includes(dsymArchive));
  // The recorded argv names a file that still exists under the evidence dir, so the command can be
  // replayed and its output compared with what this run judged.
  const recordedOut = report.symbolicatorArgv[report.symbolicatorArgv.indexOf("-o") + 1];
  assert.equal(recordedOut, join(reportDir, "symbolicated-positive", "symbolicated.json"));
  assert.ok(existsSync(recordedOut));
  assert.equal(report.checks.namedFramePresent.ok, true);
  assert.equal(report.checks.mismatchNamesNothing.ok, true);
  assert.equal(report.negative.namedFrames, 0);
  // Both invocations are handed the de-symbolicated COPY, never the report ReportCrash wrote: its
  // frames arrive named from a UUID-keyed cache that survives `strip`, and a tool handed that report
  // keeps those names (measured: 129 named frames under a MISMATCHED dSYM).
  const copyPath = join(reportDir, DESYMBOLICATED_REPORT_NAME);
  assert.equal(report.symbolicated.desymbolicatedReport, copyPath);
  assert.ok(existsSync(copyPath));
  // One ferryx frame arrived named, and it was stripped before either run.
  assert.equal(report.symbolicated.preNamedFrames, 1);
  for (const call of calls.filter((entry) => entry[0] === "/usr/bin/python3")) {
    assert.equal(call.at(-1), copyPath, "the symbolicator must be given the copy, not the raw report");
    assert.notEqual(call.at(-1), crashPath);
  }
}));

/**
 * The de-symbolication contract, on the report shape the host actually produced.
 *
 * Measured: the binary was fully stripped (`nm` showed one `T` symbol) and the RAW `.ips` still
 * carried `symbol` on all 129 ferryx frames, because ReportCrash names frames from a cache keyed by
 * image UUID and the UUID survives `strip`. Only the binary's own frames may lose their names; a
 * system frame keeps its symbol, and everything else in the document -- header line included -- has
 * to come through untouched so the copy is still a report the tool accepts.
 */
test("de-symbolication strips only the binary's frames and keeps the header", () => {
  const raw = ipsReport({
    usedImages: [
      KERNEL_IMAGE,
      { name: "ferryx", uuid: ARM64, base: 0x100000000 },
      // Undashed, lowercase: the UUID comparison must normalize, not string-match.
      { path: "/tmp/pkg/renamed-executable", uuid: ARM64.replace(/-/g, "").toLowerCase(), base: 0x100000000 },
    ],
    threads: [
      {
        frames: [
          // A named SYSTEM frame: it must survive, because naming libsystem proves nothing either way.
          { imageOffset: 100, symbol: "__pthread_kill", symbolLocation: 8, imageIndex: 0 },
          // A binary frame carrying everything ReportCrash filled in.
          {
            imageOffset: 44496588,
            symbol: "parking_lot::condvar::Condvar::wait_until_internal::h275...",
            symbolLocation: 524,
            sourceFile: "condvar.rs",
            sourceLine: 268,
            inline: false,
            imageIndex: 1,
          },
          // The same binary, reached through its undashed UUID, with no source location at all.
          { imageOffset: 512, symbol: "ferryx::daemon::server::serve", symbolLocation: 8, imageIndex: 2 },
          // An unnamed binary frame: nothing to strip, but it still had to be recognized as binary.
          { imageOffset: 640, imageIndex: 1 },
        ],
      },
    ],
  });
  const result = desymbolicateReport({ text: raw, binaryUuids: [ARM64] });
  assert.ok(result);
  // Two binary frames arrived named; that is what the report publishes for transparency.
  assert.equal(result.preNamedFrames, 2);
  // strippedFrames counts only frames that actually lost a naming field; binaryFrameCount counts
  // every frame the binary owns, including the one that arrived with nothing to strip.
  assert.equal(result.strippedFrames, 2);
  assert.equal(result.binaryFrameCount, 3);
  // The header line is preserved BYTE for BYTE: it carries the report's own timestamp and bug type.
  assert.equal(result.text.split("\n")[0], raw.split("\n")[0]);

  const copy = JSON.parse(result.text.slice(result.text.indexOf("\n") + 1));
  const [system, first, second, unnamed] = copy.threads[0].frames;
  // The system frame keeps its name and nothing about it changed.
  assert.deepEqual(system, { imageOffset: 100, symbol: "__pthread_kill", symbolLocation: 8, imageIndex: 0 });
  // Every naming field is gone from the binary's frames, and the address fields are kept verbatim.
  assert.deepEqual(first, { imageOffset: 44496588, imageIndex: 1 });
  assert.deepEqual(second, { imageOffset: 512, imageIndex: 2 });
  assert.deepEqual(unnamed, { imageOffset: 640, imageIndex: 1 });
  assert.equal("sourceLine" in first, false);
  assert.equal("inline" in first, false);
  // The images themselves are untouched: `usedImages` is what the tool keys its dSYM lookup on.
  assert.deepEqual(copy.usedImages, JSON.parse(raw.slice(raw.indexOf("\n") + 1)).usedImages);

  // A legacy text report is not JSON, and a report with no binary UUID to identify frames by cannot
  // say which image is the binary: both are left alone rather than guessed at.
  assert.equal(desymbolicateReport({ text: NAMED_REPORT, binaryUuids: [ARM64] }), null);
  assert.equal(desymbolicateReport({ text: raw, binaryUuids: [] }), null);
  assert.equal(desymbolicateReport({ text: raw, binaryUuids: [X86] }), null);
});

/**
 * The regression this fix exists for, judged end to end.
 *
 * The raw report already names the ferryx frames. The tool below adds NOTHING -- its output is its
 * input, which is the semantics measured on the host: CrashSymbolicator.py keeps the names a report
 * already carries and only fills gaps, so under the mismatched dSYM its output still held 129 named
 * frames and `mismatchNamesNothing` could never pass. Judged on the raw report the negative check
 * fails; judged on the de-symbolicated copy it passes, and the raw `.ips` is never modified.
 */
test("a pre-named raw report still fails the mismatch check only when it is not de-symbolicated", () => withTmp((root) => {
  const { crashPath, dsymArchive, binary, artifact } = collectFixture(root);
  const raw = ipsReport({
    usedImages: [KERNEL_IMAGE, { name: "ferryx", uuid: ARM64, base: 0x100000000 }],
    threads: [
      {
        frames: [
          { imageOffset: 100, symbol: "__pthread_kill", symbolLocation: 8, imageIndex: 0 },
          {
            imageOffset: 44496588,
            symbol: "parking_lot::condvar::Condvar::wait_until_internal::h275...",
            symbolLocation: 524,
            sourceFile: "condvar.rs",
            sourceLine: 268,
            imageIndex: 1,
          },
          {
            imageOffset: 640,
            symbol: "ferryx::terminal::pty::PtySession::read_frame::h1",
            symbolLocation: 32,
            sourceFile: "pty.rs",
            sourceLine: 412,
            imageIndex: 1,
          },
        ],
      },
    ],
  });
  writeFileSync(crashPath, raw);

  // The tool that adds nothing: it writes its INPUT back out, so a name in the output can only be a
  // name the report already had. That makes the input the harness chooses the only variable.
  const passThroughRuns = [];
  const run = (command, args, options = {}) => {
    if (command === "/usr/bin/python3") {
      const input = String(args.at(-1));
      passThroughRuns.push(input);
      writeFileSync(args[args.indexOf("-o") + 1], readFileSync(input, "utf8"));
      return { status: 0, stdout: "", stderr: "" };
    }
    return judgeRun({ collector: { crashReport: crashPath } })(command, args, options);
  };

  // The state this fix removes: symbolication of the RAW report names frames under a dSYM that must
  // name nothing, so the negative check could never pass however correct the archive was.
  const rawVerdict = evaluateMismatchedDsym({
    gateCode: "DSYM_UUID_MISMATCH",
    symbolicatedText: renderJsonCrashFrames({ text: raw, binaryUuids: [ARM64] }).text,
  });
  assert.equal(rawVerdict.checks.mismatchNamesNothing.ok, false);

  const reportDir = join(root, "report");
  mkdirSync(reportDir);
  const report = runSymbolicationChecks({
    binary,
    gate: fakeGate(),
    crashPath,
    dsymArchive,
    reportDir,
    packaged: artifact,
    tool: "symbolicatecrash-path",
    crashSymbolicator: CRASH_SYMBOLICATOR,
    developerDir: XCODE_DEVELOPER_DIR,
    forced: forcedRecordFor({ binary }),
    run,
  });

  // What the dSYM contributed is now the only thing judged: the tool added nothing, so the negative
  // run names nothing and the mismatch check passes.
  assert.equal(report.negative.namedFrames, 0, JSON.stringify(report.negative));
  assert.equal(report.checks.mismatchNamesNothing.ok, true);
  // The positive check still fails HERE, and correctly: this pass-through tool resolved no name, so
  // the archive is shown to have contributed nothing. The acceptance is not being loosened.
  assert.equal(report.checks.namedFramePresent.ok, false);
  assert.equal(report.verdict, "FAIL");

  // The two names ReportCrash had already filled in are recorded, so a reader can see how many
  // frames a verdict would have scored without the archive.
  assert.equal(report.symbolicated.preNamedFrames, 2);
  assert.equal(report.symbolicated.desymbolicatedFrames, 2);
  assert.equal(report.symbolicated.desymbolicatedReport, join(reportDir, DESYMBOLICATED_REPORT_NAME));

  // Both invocations read the copy, and only the copy.
  assert.deepEqual(new Set(passThroughRuns), new Set([join(reportDir, DESYMBOLICATED_REPORT_NAME)]));
  // The original .ips is untouched: the copy is the only thing written.
  assert.equal(readFileSync(crashPath, "utf8"), raw);
  // And the copy really carries no name for a binary frame while keeping the system frame's.
  const copy = JSON.parse(readFileSync(report.symbolicated.desymbolicatedReport, "utf8").split("\n")[1]);
  const [system, ...binaryOnes] = copy.threads[0].frames;
  assert.equal(system.symbol, "__pthread_kill");
  for (const frame of binaryOnes) assert.equal("symbol" in frame, false);
  // The evidence on disk states the same thing the returned report does.
  const written = JSON.parse(readFileSync(join(reportDir, "report.json"), "utf8"));
  assert.equal(written.symbolicated.preNamedFrames, 2);
  assert.equal(written.symbolicated.desymbolicatedReport, join(reportDir, DESYMBOLICATED_REPORT_NAME));
}));
