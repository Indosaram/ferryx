// Fixture writer for the crash symbolication gate (scripts/lib/release-symbolication.mjs).
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { gzipSync } from "node:zlib";

import {
  DSYM_ARCHIVE_SCHEMA,
  PACKAGED_APP_ARTIFACT,
  PACKAGED_APP_MEMBER,
  SYMBOLICATION_EVIDENCE_DIR,
  SYMBOLICATION_REPORT_NAME,
  SYMBOLICATION_SCHEMA,
} from "../lib/release-symbolication.mjs";

/** The bytes the fixture packages, so the report and the archive agree by construction. */
export const PACKAGED_CONTENT = "fixture-packaged-ferryx-binary\n";

/** Two UUIDs, matching the shape a fat/universal link of the shipped binary records. */
export const FIXTURE_UUIDS = ["1A2B3C4D-0000-1111-2222-333344445555", "6A7B8C9D-0000-1111-2222-333344445555"];

function tarHeader({ name, size }) {
  const header = Buffer.alloc(512);
  const octal = (value, length) => `${value.toString(8).padStart(length - 1, "0")}\0`;
  header.write(name, 0, 100, "utf8");
  header.write(octal(0o644, 8), 100, 8, "utf8");
  header.write(octal(0, 8), 108, 8, "utf8");
  header.write(octal(0, 8), 116, 8, "utf8");
  header.write(octal(size, 12), 124, 12, "utf8");
  header.write(octal(0, 12), 136, 12, "utf8");
  header.write("0", 156, 1, "utf8");
  header.write("ustar\0", 257, 6, "utf8");
  header.write("00", 263, 2, "utf8");
  header.write(" ".repeat(8), 148, 8, "utf8");
  let sum = 0;
  for (const byte of header) sum += byte;
  header.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148, 8, "utf8");
  return header;
}

/** Writes `artifacts/darwin/Ferryx.app.tar.gz` holding the packaged binary, and returns its sha256. */
export function writePackagedAppArtifact(releaseDir, content = PACKAGED_CONTENT, memberPath = PACKAGED_APP_MEMBER) {
  const body = Buffer.from(content);
  const padding = Buffer.alloc((512 - (body.length % 512)) % 512);
  const archive = Buffer.concat([tarHeader({ name: memberPath, size: body.length }), body, padding, Buffer.alloc(1024)]);
  const artifactPath = join(releaseDir, PACKAGED_APP_ARTIFACT);
  mkdirSync(dirname(artifactPath), { recursive: true });
  writeFileSync(artifactPath, gzipSync(archive));
  return createHash("sha256").update(body).digest("hex");
}

/** A `dsyms/manifest.json` recording the UUIDs of the shipped binary. */
export function writeDsymArchiveFixture(releaseDir, { uuids = FIXTURE_UUIDS, binary = "/run/Ferryx.app/Contents/MacOS/ferryx" } = {}) {
  const dsymDir = join(releaseDir, "dsyms");
  mkdirSync(dsymDir, { recursive: true });
  writeFileSync(
    join(dsymDir, "manifest.json"),
    JSON.stringify({
      schema: DSYM_ARCHIVE_SCHEMA,
      binaries: [{ binary, uuids, functions: 1234, dsym: "ferryx.dSYM" }],
      gate: [{ binary, uuids, dsyms: ["ferryx.dSYM"] }],
    }),
  );
  return dsymDir;
}

/** A symbolication report the gate must accept. `overrides` perturbs exactly one field. */
export function passingSymbolicationReport({ sha256, uuids = FIXTURE_UUIDS, overrides = {} } = {}) {
  return {
    schema: SYMBOLICATION_SCHEMA,
    verdict: "PASS",
    // A forced crash of this process: the only crash the gate accepts as this run's evidence.
    forcedCrash: { pid: 424242, signal: "SIGSEGV" },
    binary: { path: "/run/Ferryx.app/Contents/MacOS/ferryx", uuids, sha256 },
    dsymArchive: { dir: "/run/dsyms", uuids },
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

/** Writes `symbolication/report.json` into `releaseDir`. */
export function writeSymbolicationReport(releaseDir, report) {
  const evidenceDir = join(releaseDir, SYMBOLICATION_EVIDENCE_DIR);
  mkdirSync(evidenceDir, { recursive: true });
  writeFileSync(join(evidenceDir, SYMBOLICATION_REPORT_NAME), JSON.stringify(report));
  return evidenceDir;
}

/**
 * Writes every piece the symbolication gate reads: the packaged artifact, the dSYM archive
 * manifest, and a passing report bound to that artifact's bytes.
 */
export function writeSymbolicationFixture(releaseDir) {
  const sha256 = writePackagedAppArtifact(releaseDir);
  writeDsymArchiveFixture(releaseDir);
  writeSymbolicationReport(releaseDir, passingSymbolicationReport({ sha256 }));
}
