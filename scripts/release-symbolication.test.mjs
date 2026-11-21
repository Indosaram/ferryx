// Coverage for the Phase 0 crash-symbolication release gate (scripts/lib/release-symbolication.mjs).
//
// The gate's job is to refuse a release whose dSYM archive was never proved to name a frame. Each
// case removes one of those proofs -- the evidence file, the PASS verdict, a named check, a named
// frame, the refused negative run, and the binding between the report's UUIDs and this run's
// archive -- and requires a typed refusal. The accepted case proves the gate is satisfiable, so the
// negative cases cannot pass by the gate refusing everything.
import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { gzipSync } from "node:zlib";

const PACKAGED_MEMBER = "Ferryx.app/Contents/MacOS/ferryx";

/** sha256 of the bytes the fixture packages, so the report and the artifact agree by construction. */
function sha256Hex(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

/**
 * One tar header block.
 *
 * `prefix` and `typeflag` are explicit because the real producer uses both: `bsdtar 3.5.3` splits a
 * name longer than 100 bytes across `prefix`/`name`, and emits pax `x` records when a component
 * cannot be split at all. Fixtures that only ever wrote the 100-byte name field would pass while
 * the parser resolved the wrong path on real archives.
 */
function tarHeader({ name, prefix = "", size, typeflag = "0" }) {
  const header = Buffer.alloc(512);
  const octal = (value, length) => `${value.toString(8).padStart(length - 1, "0")}\0`;
  header.write(name, 0, 100, "utf8");
  header.write(octal(0o644, 8), 100, 8, "utf8");
  header.write(octal(0, 8), 108, 8, "utf8");
  header.write(octal(0, 8), 116, 8, "utf8");
  header.write(octal(size, 12), 124, 12, "utf8");
  header.write(octal(0, 12), 136, 12, "utf8");
  header.write(typeflag, 156, 1, "utf8");
  header.write(prefix, 345, 155, "utf8");
  header.write("ustar\0", 257, 6, "utf8");
  header.write("00", 263, 2, "utf8");
  // The checksum is computed over the header with its own field read as eight spaces.
  header.write(" ".repeat(8), 148, 8, "utf8");
  let sum = 0;
  for (const byte of header) sum += byte;
  header.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148, 8, "utf8");
  return header;
}

/** A single archive member: header, body, and the zero padding up to the next block. */
function tarMember({ name, prefix = "", typeflag = "0", content = "" }) {
  const body = Buffer.isBuffer(content) ? content : Buffer.from(content);
  const padding = Buffer.alloc((512 - (body.length % 512)) % 512);
  return Buffer.concat([tarHeader({ name, prefix, size: body.length, typeflag }), body, padding]);
}

/**
 * Writes a `.tar.gz` at `artifactPath` from explicit member blocks.
 *
 * Hand-built rather than shelling out to `tar`: the packing decision under test is a checksum, an
 * exact member name, and a duplicate check, so the bytes must be identical on every machine and
 * every run. `endMarker: false` omits the two zero blocks, which is how a cut-short archive looks.
 */
function writeArtifact(artifactPath, members, { endMarker = true } = {}) {
  const archive = Buffer.concat([...members, ...(endMarker ? [Buffer.alloc(1024)] : [])]);
  mkdirSync(dirname(artifactPath), { recursive: true });
  writeFileSync(artifactPath, gzipSync(archive));
}

/**
 * Writes the ordinary case: one member at the exact path the producer creates.
 *
 * The producer is `tar --no-xattrs -czf <run>/artifacts/darwin/Ferryx.app.tar.gz -C <dir> Ferryx.app`
 * with `COPYFILE_DISABLE=1`, which yields bare `Ferryx.app/...` names and no AppleDouble siblings.
 */
function writePackagedArtifact(artifactPath, content, memberPath = PACKAGED_MEMBER) {
  writeArtifact(artifactPath, [tarMember({ name: memberPath, content })]);
  return sha256Hex(Buffer.from(content));
}

import {
  DSYM_ARCHIVE_SCHEMA,
  SYMBOLICATION_EVIDENCE_DIR,
  SYMBOLICATION_REPORT_NAME,
  SYMBOLICATION_SCHEMA,
  SymbolicationGateError,
  assertSymbolicationGate,
  buildDsymArchiveManifest,
  evaluateSymbolicationEvidence,
  writeDsymArchiveManifest,
} from "./lib/release-symbolication.mjs";

const BINARY_UUIDS = ["1A2B3C4D-0000-1111-2222-333344445555", "6A7B8C9D-0000-1111-2222-333344445555"];

/**
 * Bytes the fixture ships as the packaged binary, and their hash.
 *
 * The gate binds evidence to the artifact by bytes, so a fixture that only matched UUIDs would test
 * the old, weaker rule. These constants make the packaged member and the report agree by
 * construction, and every byte-level case below perturbs exactly one of them.
 */
const PACKAGED_CONTENT = "fixture-packaged-ferryx-binary\n";
const PACKAGED_SHA = sha256Hex(Buffer.from(PACKAGED_CONTENT));

function fixture(
  t,
  { packagedContent = PACKAGED_CONTENT, memberPath = PACKAGED_MEMBER, writeArtifact: write = null, artifactName = null } = {},
) {
  const root = mkdtempSync(join(tmpdir(), "symbolication-gate-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  // Every run directory ships an artifact; the gate requires the accepted binary to be inside it.
  const artifactPath = join(root, "artifacts", "darwin", "Ferryx.app.tar.gz");
  if (write) write(artifactPath);
  else if (artifactName) writeArtifact(artifactPath, [tarMember({ name: artifactName, content: packagedContent })]);
  else writePackagedArtifact(artifactPath, packagedContent, memberPath);
  return root;
}

/** A run directory with a dSYM archive manifest recording the shipped binary's UUIDs. */
function writeArchive(releaseDir, { uuids = BINARY_UUIDS, binary = "/run/Ferryx.app/Contents/MacOS/ferryx" } = {}) {
  mkdirSync(join(releaseDir, "dsyms"), { recursive: true });
  writeFileSync(
    join(releaseDir, "dsyms", "manifest.json"),
    JSON.stringify({
      schema: "ferryx-dsym-archive/v1",
      binaries: [{ binary, uuids, functions: 1234, dsym: "ferryx.dSYM" }],
      gate: [{ binary, uuids, dsyms: ["ferryx.dSYM"] }],
    }),
  );
}

/** A harness report that the gate must accept. */
function passingReport(overrides = {}) {
  return {
    schema: SYMBOLICATION_SCHEMA,
    verdict: "PASS",
    // A forced crash of this process: the only crash the gate accepts as this run's evidence.
    forcedCrash: { pid: 424242, signal: "SIGSEGV" },
    binary: { path: "/run/Ferryx.app/Contents/MacOS/ferryx", uuids: BINARY_UUIDS, sha256: PACKAGED_SHA },
    dsymArchive: { dir: "/run/dsyms", uuids: BINARY_UUIDS },
    symbolicated: { frames: 18, binaryFrames: 9, namedFrames: 7, anonymousFrames: 2 },
    negative: { gateCode: "DSYM_UUID_MISMATCH", frames: 18, namedFrames: 0 },
    checks: {
      crashReportNamesBinary: { ok: true, detail: "binary path present" },
      namedFramePresent: { ok: true, detail: "7 named binary frames" },
      negativeGateRefused: { ok: true, detail: "DSYM_UUID_MISMATCH" },
      negativeNamedNothing: { ok: true, detail: "0 named frames" },
    },
    ...overrides,
  };
}

function writeReport(releaseDir, report) {
  const dir = join(releaseDir, SYMBOLICATION_EVIDENCE_DIR);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, SYMBOLICATION_REPORT_NAME), typeof report === "string" ? report : JSON.stringify(report));
  return dir;
}

function refusal(code, fn) {
  assert.throws(fn, (error) => {
    assert.ok(
      error instanceof SymbolicationGateError,
      `expected SymbolicationGateError, got ${error?.name}: ${error?.message}`,
    );
    assert.equal(error.code, code, `expected ${code}, got ${error.code}: ${error.message}`);
    return true;
  });
}

test("a passing report bound to this run's archive is accepted", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport());

  const { result, reportPath } = assertSymbolicationGate({ releaseDir });
  assert.equal(result.ok, true);
  assert.deepEqual(result.archived, BINARY_UUIDS);
  assert.equal(result.report.verdict, "PASS");

  // The gate leaves a human-readable summary beside the evidence it judged.
  const summary = readFileSync(reportPath, "utf8");
  assert.match(summary, /Crash symbolication/);
  assert.match(summary, /DSYM_UUID_MISMATCH/);
  assert.doesNotMatch(summary, /Blocking findings/);
});

test("missing evidence is a refusal, never a skip", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  // No symbolication/report.json: the run never proved the archive works.
  refusal("SYMBOLICATION_EVIDENCE_MISSING", () => assertSymbolicationGate({ releaseDir }));
});

test("unreadable and unsupported evidence is refused", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  writeReport(releaseDir, "{ not json");
  refusal("SYMBOLICATION_EVIDENCE_INVALID", () => assertSymbolicationGate({ releaseDir }));

  writeReport(releaseDir, passingReport({ schema: "ferryx-dsym-symbolication/0" }));
  refusal("SYMBOLICATION_SCHEMA_UNSUPPORTED", () => assertSymbolicationGate({ releaseDir }));
});

test("a failed verdict is refused even when the checks look fine", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport({ verdict: "FAIL" }));
  refusal("SYMBOLICATION_REJECTED", () => assertSymbolicationGate({ releaseDir }));
});

test("a vacuous report with no checks is refused", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  // A verdict with nothing behind it: exactly the acceptance this gate exists to prevent.
  writeReport(releaseDir, passingReport({ checks: {} }));
  refusal("SYMBOLICATION_CHECKS_MISSING", () => assertSymbolicationGate({ releaseDir }));
});

test("each failed named check is refused", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  writeReport(
    releaseDir,
    passingReport({
      checks: {
        crashReportNamesBinary: { ok: false, detail: "crash report has no binary line" },
        namedFramePresent: { ok: true },
        negativeGateRefused: { ok: true },
        negativeNamedNothing: { ok: true },
      },
    }),
  );
  const { blocking } = evaluateSymbolicationEvidence({ releaseDir });
  // The finding names the check that failed, so an operator does not have to open the report.
  const failed = blocking.find((p) => p.code === "SYMBOLICATION_CHECK_FAILED");
  assert.ok(failed, "expected a SYMBOLICATION_CHECK_FAILED finding");
  assert.match(failed.detail, /crashReportNamesBinary/);
  refusal("SYMBOLICATION_CHECK_FAILED", () => assertSymbolicationGate({ releaseDir }));
});

test("PASS with zero named frames is refused on the number, not the verdict", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  writeReport(
    releaseDir,
    passingReport({ symbolicated: { frames: 4, binaryFrames: 4, namedFrames: 0, anonymousFrames: 4 } }),
  );
  refusal("SYMBOLICATION_NO_NAMED_FRAME", () => assertSymbolicationGate({ releaseDir }));
});

test("a negative run that was not refused for a UUID mismatch is refused", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  // The gate must have refused the mismatched dSYM specifically; "no dSYM at all" proves nothing.
  writeReport(releaseDir, passingReport({ negative: { gateCode: "DSYM_MISSING", frames: 0, namedFrames: 0 } }));
  refusal("SYMBOLICATION_NEGATIVE_NOT_REFUSED", () => assertSymbolicationGate({ releaseDir }));
});

test("a negative run that named frames under a mismatched dSYM is refused", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport({ negative: { gateCode: "DSYM_UUID_MISMATCH", frames: 9, namedFrames: 3 } }));
  refusal("SYMBOLICATION_NEGATIVE_NAMED", () => assertSymbolicationGate({ releaseDir }));
});

test("evidence for other artifacts is refused even with a PASS verdict", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  // A passing report from a different build: pairing it with these artifacts would ship a dSYM
  // nobody symbolicated.
  writeReport(
    releaseDir,
    passingReport({ binary: { path: "/other/Ferryx.app/Contents/MacOS/ferryx", uuids: ["FFFFFFFF-0000-1111-2222-333344445555"] } }),
  );
  refusal("SYMBOLICATION_UUID_NOT_ARCHIVED", () => assertSymbolicationGate({ releaseDir }));
});

test("an archive with no recorded UUID cannot vouch for anything", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir, { uuids: [] });
  writeReport(releaseDir, passingReport());
  refusal("SYMBOLICATION_ARCHIVE_UNRECORDED", () => assertSymbolicationGate({ releaseDir }));
});

test("a missing archive manifest cannot vouch for anything", (t) => {
  const releaseDir = fixture(t);
  writeReport(releaseDir, passingReport());
  refusal("SYMBOLICATION_ARCHIVE_UNRECORDED", () => assertSymbolicationGate({ releaseDir }));
});

/**
 * The local notarized build (`scripts/build-notarized-local.mjs`) archived dSYMs but wrote no
 * manifest, so every run produced that way could only fail the gate. These cases pin the writer
 * that closes that gap, and prove its output satisfies the gate the pipeline actually enforces.
 */
test("the archive manifest writer records what the gate needs to bind evidence", (t) => {
  const releaseDir = fixture(t);
  const dsymDir = join(releaseDir, "dsyms");
  const manifest = buildDsymArchiveManifest({
    runDir: releaseDir,
    runId: "20261116-1",
    tag: "v2026.1116.1",
    appVersion: "2026.1116.1",
    binaries: [
      { binary: join(releaseDir, "Ferryx.app/Contents/MacOS/ferryx"), bundle: join(dsymDir, "ferryx.dSYM"), uuids: BINARY_UUIDS, functions: { arm64: 1000, x86_64: 900 } },
    ],
    gates: [
      { binaryPath: join(releaseDir, "Ferryx.app/Contents/MacOS/ferryx"), binaryUuids: BINARY_UUIDS, bundles: [{ path: join(dsymDir, "ferryx.dSYM") }] },
    ],
    keeper: { version: "2026.1116.1", protocolMajor: 1, protocolMinor: 0, binary: "keeper/ferryx-ptykeeper", sha256: "a".repeat(64) },
    createdAt: "2026-11-16T00:00:00.000Z",
  });
  assert.equal(manifest.schema, DSYM_ARCHIVE_SCHEMA);
  // Paths under the run dir are stored relative, so the manifest survives the run being tarred.
  assert.equal(manifest.binaries[0].dsym, "dsyms/ferryx.dSYM");
  assert.equal(manifest.gate[0].dsyms[0], "dsyms/ferryx.dSYM");
  assert.deepEqual(manifest.gate[0].uuids, BINARY_UUIDS);

  const manifestPath = writeDsymArchiveManifest(dsymDir, manifest);
  assert.equal(manifestPath, join(dsymDir, "manifest.json"));
  assert.deepEqual(JSON.parse(readFileSync(manifestPath, "utf8")), manifest);

  // The gate reads the written manifest and accepts a matching report: the writer is not vacuous.
  writeReport(releaseDir, passingReport());
  const { result } = assertSymbolicationGate({ releaseDir });
  assert.equal(result.ok, true);
  assert.deepEqual(result.archived, BINARY_UUIDS);
});

test("the archive manifest writer creates the archive dir and omits an absent keeper", (t) => {
  const releaseDir = fixture(t);
  const dsymDir = join(releaseDir, "absent", "dsyms");
  const manifest = buildDsymArchiveManifest({ binaries: [{ binary: "/b", uuids: BINARY_UUIDS, dsym: "/d.dSYM" }] });
  assert.equal("keeper" in manifest, false);
  // With no run dir there is nothing to relativize against, so absolute paths are kept.
  assert.equal(manifest.binaries[0].dsym, "/d.dSYM");
  writeDsymArchiveManifest(dsymDir, manifest);
  assert.ok(readFileSync(join(dsymDir, "manifest.json"), "utf8").includes(DSYM_ARCHIVE_SCHEMA));
});

// ---------------------------------------------------------------------------------------------
// Skip-proofing: a supplied crash report is not a forced crash, and matching UUIDs are not the
// shipped bytes. Each case defeats one way of satisfying the earlier checks without proving
// anything about this build's binary.
// ---------------------------------------------------------------------------------------------

test("a report with no forced crash is refused, even with a PASS verdict", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  // `--ips other.ips`: an operator supplied a historical crash report. It may name a frame, but
  // nothing shows this run produced it, so it cannot stand as Phase 0's evidence.
  writeReport(releaseDir, passingReport({ forcedCrash: null }));
  refusal("SYMBOLICATION_CRASH_NOT_FORCED", () => assertSymbolicationGate({ releaseDir }));
});

test("a crash that was not a SIGSEGV is refused", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  // A clean exit or a different signal is not the crash the plan asks the harness to force, so a
  // report claiming one is describing some other event.
  writeReport(releaseDir, passingReport({ forcedCrash: { pid: 424242, signal: "SIGTERM" } }));
  refusal("SYMBOLICATION_CRASH_SIGNAL", () => assertSymbolicationGate({ releaseDir }));
});

test("a forced crash with no usable pid is refused", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport({ forcedCrash: { pid: null, signal: "SIGSEGV" } }));
  refusal("SYMBOLICATION_CRASH_PID_INVALID", () => assertSymbolicationGate({ releaseDir }));
});

test("a report whose binary bytes are not the packaged bytes is refused", (t) => {
  // The gate's core claim: the symbolicated binary IS the shipped binary. Here the archive matches
  // by UUID and every check passes, but the report describes different bytes than the artifact
  // carries -- exactly a locally built binary symbolicated in place of the packaged one.
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  writeReport(
    releaseDir,
    passingReport({ binary: { path: "/run/Ferryx.app/Contents/MacOS/ferryx", uuids: BINARY_UUIDS, sha256: "b".repeat(64) } }),
  );
  refusal("SYMBOLICATION_BINARY_NOT_SHIPPED", () => assertSymbolicationGate({ releaseDir }));
});

test("a run with no packaged artifact cannot bind its evidence", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  rmSync(join(releaseDir, "artifacts"), { recursive: true, force: true });
  writeReport(releaseDir, passingReport());
  refusal("SYMBOLICATION_ARTIFACT_MISSING", () => assertSymbolicationGate({ releaseDir }));
});

test("a packaged artifact without the expected member is refused", (t) => {
  // The tarball exists, so it is not a missing-artifact case; the member the runbook extracts is
  // simply not in it, which means the binary under acceptance came from somewhere else.
  const releaseDir = fixture(t, { artifactName: "Ferryx.app/Contents/MacOS/not-ferryx" });
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport());
  refusal("SYMBOLICATION_ARTIFACT_UNREADABLE", () => assertSymbolicationGate({ releaseDir }));
});

test("an unreadable packaged artifact is refused as unreadable", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  writeFileSync(join(releaseDir, "artifacts", "darwin", "Ferryx.app.tar.gz"), "not gzip");
  writeReport(releaseDir, passingReport());
  refusal("SYMBOLICATION_ARTIFACT_UNREADABLE", () => assertSymbolicationGate({ releaseDir }));
});

test("a report with no hash and no readable binary path is refused", (t) => {
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  // Neither `binary.sha256` nor a hashable `binary.path`: there is no way to say which bytes were
  // symbolicated, so the evidence is refused rather than assumed to be the packaged binary.
  writeReport(releaseDir, passingReport({ binary: { path: "/nonexistent/ferryx", uuids: BINARY_UUIDS } }));
  refusal("SYMBOLICATION_BINARY_UNHASHED", () => assertSymbolicationGate({ releaseDir }));
});

test("a report without an acceptance-time binary.sha256 is refused even if the path still hashes", (t) => {
  // The binary named in the report is present on disk and hashes to the packaged bytes, so a gate
  // that fell back to re-hashing the path would accept this. It must still refuse: a digest taken
  // now, after the crash, cannot prove which bytes ran -- the file could have been replaced in
  // between. Only the digest captured at acceptance time closes that gap.
  const releaseDir = fixture(t);
  writeArchive(releaseDir);
  const acceptedPath = join(releaseDir, "Ferryx.app", "Contents", "MacOS", "ferryx");
  mkdirSync(dirname(acceptedPath), { recursive: true });
  writeFileSync(acceptedPath, PACKAGED_CONTENT);
  writeReport(releaseDir, passingReport({ binary: { path: acceptedPath, uuids: BINARY_UUIDS } }));
  refusal("SYMBOLICATION_BINARY_UNHASHED", () => assertSymbolicationGate({ releaseDir }));
});

// ---------------------------------------------------------------------------------------------
// Archive format: these mirror what `bsdtar 3.5.3` (libarchive 3.7.4, this workstation's tar)
// actually emits, verified by packing a fixture app and reading the raw headers back. A parser
// that handled only the 100-byte name field would resolve the wrong member on real artifacts.
// ---------------------------------------------------------------------------------------------

test("a member split across the ustar prefix field is found", (t) => {
  // bsdtar splits a name longer than 100 bytes: prefix holds the leading directories, name the
  // tail. Joining them is what makes the real archive's member addressable at all.
  const releaseDir = fixture(t, {
    writeArtifact: (artifactPath) =>
      writeArtifact(artifactPath, [
        tarMember({ prefix: "Ferryx.app/Contents/MacOS", name: "ferryx", content: PACKAGED_CONTENT }),
      ]),
  });
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport());
  const { result } = assertSymbolicationGate({ releaseDir });
  assert.equal(result.ok, true);
  assert.equal(result.packagedSha256, PACKAGED_SHA);
});

test("a member named by a pax extended header is found", (t) => {
  // A component too long to split (bsdtar emits this for a >100-byte directory) becomes a `type x`
  // record whose body carries `path=` for the entry that follows. Ignoring it would resolve the
  // truncated inline name and find nothing.
  const releaseDir = fixture(t, {
    writeArtifact: (artifactPath) => {
      const paxBody = `30 path=${PACKAGED_MEMBER}\n`;
      writeArtifact(artifactPath, [
        tarMember({ name: "PaxHeader/Ferryx.app", typeflag: "x", content: paxBody }),
        // The inline name is deliberately wrong: only the pax record names the real member.
        tarMember({ name: "Ferryx.app/Contents/MacOS/truncated", content: PACKAGED_CONTENT }),
      ]);
    },
  });
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport());
  const { result } = assertSymbolicationGate({ releaseDir });
  assert.equal(result.ok, true);
  assert.equal(result.packagedSha256, PACKAGED_SHA);
});

test("a member named by a GNU long-name record is found", (t) => {
  // `tar -C dir .` from GNU tar, and any repack on Linux, uses `L` instead of pax.
  const releaseDir = fixture(t, {
    writeArtifact: (artifactPath) => {
      writeArtifact(artifactPath, [
        tarMember({ name: "././@LongLink", typeflag: "L", content: `${PACKAGED_MEMBER}\0` }),
        tarMember({ name: "Ferryx.app/Contents/MacOS/truncated", content: PACKAGED_CONTENT }),
      ]);
    },
  });
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport());
  const { result } = assertSymbolicationGate({ releaseDir });
  assert.equal(result.ok, true);
  assert.equal(result.packagedSha256, PACKAGED_SHA);
});

test("a ./ prefixed archive is matched after normalizing the prefix", (t) => {
  // The runbook extracts with `tar -xzf`, but an operator who repacks with `-C dir .` gets `./`
  // names. The member is the same file, so it must still be found; matching stays exact otherwise.
  const releaseDir = fixture(t, { artifactName: `./${PACKAGED_MEMBER}` });
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport());
  const { result } = assertSymbolicationGate({ releaseDir });
  assert.equal(result.ok, true);
  assert.equal(result.packagedSha256, PACKAGED_SHA);
});

test("a member truncated by a cut-short archive is refused, not silently shortened", (t) => {
  // The header claims more bytes than the archive holds. Slicing past the buffer yields a short
  // body, which would hash differently and be reported as a bogus "not shipped" mismatch; the
  // real problem is a damaged artifact and must be named as such.
  const releaseDir = fixture(t, {
    writeArtifact: (artifactPath) => {
      const member = tarMember({ name: PACKAGED_MEMBER, content: PACKAGED_CONTENT });
      // Keep the header and a few body bytes, drop the rest and the end-of-archive marker.
      writeArtifact(artifactPath, [member.subarray(0, 512 + 8)], { endMarker: false });
    },
  });
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport());
  refusal("SYMBOLICATION_ARTIFACT_TRUNCATED", () => assertSymbolicationGate({ releaseDir }));
});

test("an archive with no end-of-archive marker is refused even when the member reads cleanly", (t) => {
  // Every member decoded, but the archive never terminated: a later member may have been cut off
  // by the same damage, so the member we found cannot be assumed to be the one that ships.
  const releaseDir = fixture(t, {
    writeArtifact: (artifactPath) =>
      writeArtifact(artifactPath, [tarMember({ name: PACKAGED_MEMBER, content: PACKAGED_CONTENT })], {
        endMarker: false,
      }),
  });
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport());
  refusal("SYMBOLICATION_ARTIFACT_TRUNCATED", () => assertSymbolicationGate({ releaseDir }));
});

test("an archive with the same member path twice is refused", (t) => {
  // Extraction lets the last member win, and `find()` takes the first: the two disagree, so a
  // tarball carrying two `ferryx` entries could hash one set of bytes while the runbook extracts
  // the other. Refusing names the ambiguity instead of picking a side.
  const releaseDir = fixture(t, {
    writeArtifact: (artifactPath) =>
      writeArtifact(artifactPath, [
        tarMember({ name: PACKAGED_MEMBER, content: PACKAGED_CONTENT }),
        tarMember({ name: PACKAGED_MEMBER, content: "a different binary entirely\n" }),
      ]),
  });
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport());
  refusal("SYMBOLICATION_ARTIFACT_DUPLICATE", () => assertSymbolicationGate({ releaseDir }));
});

test("a corrupt header checksum is refused rather than yielding a garbage member", (t) => {
  const releaseDir = fixture(t, {
    writeArtifact: (artifactPath) => {
      const member = Buffer.from(tarMember({ name: PACKAGED_MEMBER, content: PACKAGED_CONTENT }));
      member[148] = member[148] === 0x30 ? 0x31 : 0x30; // perturb the recorded checksum
      writeArtifact(artifactPath, [member]);
    },
  });
  writeArchive(releaseDir);
  writeReport(releaseDir, passingReport());
  refusal("SYMBOLICATION_ARTIFACT_UNREADABLE", () => assertSymbolicationGate({ releaseDir }));
});
