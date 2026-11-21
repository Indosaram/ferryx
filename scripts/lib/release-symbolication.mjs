// Phase 0 release gate: the archived dSYM must actually symbolicate the shipped binary, and a
// mismatched dSYM must be refused.
//
// `release-dsym.mjs` proves correspondence (a dSYM carries the built binary's UUID). That is
// necessary and not sufficient: the plan's acceptance is "symbolicate the release binary's forced
// SIGSEGV and get a named function", plus a negative run the gate rejects. Only a real crash put
// through `symbolicatecrash` proves a frame gets a name, so that work happens in
// `scripts/qa/dsym-symbolication-acceptance.mjs` and this module judges its evidence.
//
// The split keeps the release gate deterministic and testable without a Mac crash: the harness
// produces `symbolication/report.json` in the run directory, and every release that publishes
// requires that report to pass. A missing report is a refusal, never a skip -- an unproven dSYM
// archive is the exact state Phase 0 exists to end.
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gunzipSync } from "node:zlib";

/** Matches `SYMBOLICATION_SCHEMA` in `scripts/lib/dsym-symbolication.mjs`. */
export const SYMBOLICATION_SCHEMA = "ferryx-dsym-symbolication/1";
/** Evidence directory name inside a release run directory. */
export const SYMBOLICATION_EVIDENCE_DIR = "symbolication";
export const SYMBOLICATION_REPORT_NAME = "report.json";
export const SYMBOLICATION_SUMMARY_NAME = "symbolication-report.md";

/**
 * Schema of `dsyms/manifest.json`: the archive describing itself.
 *
 * Written by BOTH macOS build paths. `archivedBinaryUuids` below reads it to bind symbolication
 * evidence to the artifacts that ship, and `release-local` reads `keeper` out of it for provenance;
 * a path that archives dSYMs without writing this file leaves an archive no gate can bind to.
 */
export const DSYM_ARCHIVE_SCHEMA = "ferryx-dsym-archive/v1";

/**
 * The artifact whose executable is the one users run, and the member inside it to bind to.
 *
 * `release-platforms.mjs` writes this tarball for the updater; the runbook extracts exactly this
 * member to run the acceptance harness. Binding the evidence to these bytes is what makes the gate
 * about the SHIPPED binary rather than about whatever binary happened to be under acceptance.
 */
export const PACKAGED_APP_ARTIFACT = join("artifacts", "darwin", "Ferryx.app.tar.gz");
export const PACKAGED_APP_MEMBER = "Ferryx.app/Contents/MacOS/ferryx";

/** The only signal that proves a crash: anything else is a clean exit wearing a crash's name. */
export const REQUIRED_CRASH_SIGNAL = "SIGSEGV";

/** Typed refusal: `code` leads the message and is the first blocking finding. */
export class SymbolicationGateError extends Error {
  constructor(code, message, problems = []) {
    super(`${code}: ${message}`);
    this.name = "SymbolicationGateError";
    this.code = code;
    this.problems = problems;
  }
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return null;
  }
}

export function sha256OfBytes(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

/** A packaged artifact that cannot be trusted to name one set of bytes. */
export class TarFormatError extends Error {
  constructor(message, code) {
    super(message);
    this.name = "TarFormatError";
    this.code = code;
  }
}

const TAR_BLOCK = 512;

/**
 * Strips the `./` prefix `bsdtar` writes when it is given `.` rather than a directory name.
 *
 * The release producer passes `Ferryx.app`, so shipped archives carry bare names. Normalizing keeps
 * the member lookup working for a runbook-style `tar -C dir .` repack without matching by suffix.
 */
function normalizeMemberPath(path) {
  return String(path).replace(/^(\.\/)+/, "");
}

/**
 * Parses a tar archive into `{ name, typeflag, body }` entries.
 *
 * Written against what `bsdtar 3.5.3` (libarchive 3.7.4) actually emits for the release tarball,
 * not against the ustar ideal: it splits names over 100 chars into `prefix`/`name`, writes pax
 * `typeflag "x"` records carrying `path=` for components that cannot be split, and uses GNU `L`
 * for a long name from other producers. A parser that ignored those would resolve the wrong name
 * or, worse, hash a header's worth of the wrong bytes.
 *
 * Two properties matter more than coverage, because this decides which bytes get hashed:
 * a member whose declared size runs past the buffer is refused rather than silently truncated,
 * and a path that appears twice in one archive is refused because extraction order then decides
 * which bytes ship. The header checksum is verified so a corrupt archive fails as corrupt instead
 * of as a mysterious hash mismatch two steps later.
 */
function parseTarEntries(bytes) {
  const entries = [];
  let offset = 0;
  let pendingPath = null;
  let terminated = false;
  const seen = new Map();

  const text = (header, start, length) => header.subarray(start, start + length).toString("utf8").replace(/\0[\s\S]*$/, "");
  const number = (header, start, length, label) => {
    const raw = header.subarray(start, start + length);
    // A high bit marks a base-256 field, used for sizes past 8 GiB. Refusing beats misreading: a
    // wrong size here would slice the member at the wrong offset and hash the wrong bytes.
    if (raw.length > 0 && (raw[0] & 0x80) !== 0) {
      throw new TarFormatError(`base-256 ${label} is not supported`, "TAR_UNSUPPORTED");
    }
    const digits = raw.toString("utf8").replace(/[\0 ]/g, "");
    if (digits === "") return 0;
    const value = Number.parseInt(digits, 8);
    if (!Number.isFinite(value)) {
      throw new TarFormatError(`unreadable ${label} ${JSON.stringify(digits)}`, "TAR_CORRUPT");
    }
    return value;
  };

  while (offset + TAR_BLOCK <= bytes.length) {
    const header = bytes.subarray(offset, offset + TAR_BLOCK);
    if (header.every((byte) => byte === 0)) {
      terminated = true;
      break;
    }
    const name = text(header, 0, 100);
    const prefix = text(header, 345, 155);
    const size = number(header, 124, 12, "member size");
    const typeflag = text(header, 156, 1) || "0";
    const declared = number(header, 148, 8, "header checksum");
    const recalculated = Buffer.from(header).fill(0x20, 148, 156).reduce((sum, byte) => sum + byte, 0);
    if (declared !== recalculated) {
      throw new TarFormatError(
        `corrupt tar archive: header checksum ${recalculated} does not match recorded ${declared}`,
        "TAR_CORRUPT",
      );
    }
    const bodyStart = offset + TAR_BLOCK;
    const bodyEnd = bodyStart + size;
    if (bodyEnd > bytes.length) {
      throw new TarFormatError(
        `truncated member ${JSON.stringify(normalizeMemberPath(prefix ? `${prefix}/${name}` : name))}: ` +
          `header declares ${size} bytes but only ${bytes.length - bodyStart} remain`,
        "TAR_TRUNCATED",
      );
    }
    const body = bytes.subarray(bodyStart, bodyEnd);
    if (typeflag === "L") {
      // GNU long name: the body is the real name of the entry that follows.
      pendingPath = body.toString("utf8").replace(/\0[\s\S]*$/, "");
    } else if (typeflag === "x" || typeflag === "X") {
      // BSD/pax extended header: `"<len> path=<name>\n"` overrides the next entry's name.
      const matched = /(?:^|\n)\d+ path=([\s\S]*?)\n/.exec(body.toString("utf8"));
      if (matched) pendingPath = matched[1];
    } else if (typeflag !== "g") {
      const resolved = normalizeMemberPath(pendingPath ?? (prefix ? `${prefix}/${name}` : name));
      pendingPath = null;
      if (resolved && typeflag !== "5") {
        const previous = seen.get(resolved);
        if (previous !== undefined) {
          throw new TarFormatError(
            `duplicate member ${JSON.stringify(resolved)} at blocks ${previous} and ${offset}: ` +
              "extraction order would decide which bytes ship",
            "TAR_DUPLICATE",
          );
        }
        seen.set(resolved, offset);
        entries.push({ name: resolved, typeflag, body, offset });
      }
    }
    offset = bodyStart + Math.ceil(size / TAR_BLOCK) * TAR_BLOCK;
  }
  if (!terminated) {
    throw new TarFormatError(
      "archive has no end-of-archive marker, so a member may be cut short",
      "TAR_TRUNCATED",
    );
  }
  return entries;
}

/**
 * Returns the bytes of `memberPath` inside `artifactPath`, or `null` when the archive or member is
 * absent. Absence is reported rather than thrown so the caller can name the missing thing.
 */
export function readPackagedMember({ artifactPath, memberPath = PACKAGED_APP_MEMBER }) {
  if (!artifactPath || !existsSync(artifactPath)) return null;
  let raw;
  try {
    raw = gunzipSync(readFileSync(artifactPath));
  } catch (error) {
    return { error: `cannot decompress ${artifactPath}: ${error.message}` };
  }
  let entries;
  try {
    entries = parseTarEntries(raw);
  } catch (error) {
    if (error instanceof TarFormatError) {
      const code =
        error.code === "TAR_TRUNCATED"
          ? "SYMBOLICATION_ARTIFACT_TRUNCATED"
          : error.code === "TAR_DUPLICATE"
            ? "SYMBOLICATION_ARTIFACT_DUPLICATE"
            : "SYMBOLICATION_ARTIFACT_UNREADABLE";
      return { error: `${artifactPath}: ${error.message}`, code };
    }
    throw error;
  }
  const wanted = normalizeMemberPath(memberPath);
  const entry = entries.find((item) => item.name === wanted);
  if (!entry) return { error: `${artifactPath} has no ${memberPath} member` };
  return { bytes: entry.body, memberPath, artifactPath, sha256: sha256OfBytes(entry.body) };
}

/** UUIDs the shipped `ferryx` binary is recorded under by the dSYM archive manifest. */
export function archivedBinaryUuids(dsymManifest) {
  const gate = Array.isArray(dsymManifest?.gate) ? dsymManifest.gate : [];
  const entry = gate.find((item) => /(^|[/\\])ferryx$/.test(String(item?.binary ?? "")));
  if (!entry || !Array.isArray(entry.uuids)) return [];
  return entry.uuids.filter((uuid) => typeof uuid === "string" && uuid.length > 0);
}

/**
 * Judges the symbolication evidence without throwing. `problems` lists every finding so an
 * operator sees the whole state, and `blocking` is what refuses the release.
 */
export function evaluateSymbolicationEvidence({ releaseDir, readMember = readPackagedMember }) {
  const evidenceDir = join(releaseDir, SYMBOLICATION_EVIDENCE_DIR);
  const reportPath = join(evidenceDir, SYMBOLICATION_REPORT_NAME);
  const problems = [];
  if (!existsSync(reportPath)) {
    problems.push({
      code: "SYMBOLICATION_EVIDENCE_MISSING",
      detail: `${reportPath} does not exist; run scripts/qa/dsym-symbolication-acceptance.mjs against the built artifacts and its dSYM archive`,
    });
    return { ok: false, reportPath, report: null, archived: [], problems, blocking: problems };
  }
  const report = readJson(reportPath);
  if (!report || typeof report !== "object") {
    problems.push({ code: "SYMBOLICATION_EVIDENCE_INVALID", detail: `${reportPath} is not JSON` });
    return { ok: false, reportPath, report: null, archived: [], problems, blocking: problems };
  }
  if (report.schema !== SYMBOLICATION_SCHEMA) {
    problems.push({
      code: "SYMBOLICATION_SCHEMA_UNSUPPORTED",
      detail: `${reportPath} declares schema ${JSON.stringify(report.schema)}, expected ${SYMBOLICATION_SCHEMA}`,
    });
  }
  if (report.verdict !== "PASS") {
    problems.push({
      code: "SYMBOLICATION_REJECTED",
      detail: `verdict is ${JSON.stringify(report.verdict ?? null)}, not PASS`,
    });
  }

  // Every named check the harness recorded must pass. An empty check map proves nothing, so it is
  // refused too: a "PASS" with no checks would be a vacuous acceptance.
  const checks = report.checks && typeof report.checks === "object" ? report.checks : {};
  const checkNames = Object.keys(checks);
  if (checkNames.length === 0) {
    problems.push({ code: "SYMBOLICATION_CHECKS_MISSING", detail: `${reportPath} records no named checks` });
  }
  for (const name of checkNames) {
    if (checks[name]?.ok !== true) {
      problems.push({
        code: "SYMBOLICATION_CHECK_FAILED",
        detail: `${name}: ${checks[name]?.detail ?? "failed"}`,
      });
    }
  }

  // The positive half must name a frame of the binary itself with a non-zero count. A report that
  // claims PASS but resolved nothing is rejected on the number, not on its own verdict field.
  const namedFrames = report.symbolicated?.namedFrames;
  if (!Number.isInteger(namedFrames) || namedFrames <= 0) {
    problems.push({
      code: "SYMBOLICATION_NO_NAMED_FRAME",
      detail: `namedFrames is ${JSON.stringify(namedFrames ?? null)}; the archived dSYM named no frame of the binary`,
    });
  }

  // The negative half: the real gate must have refused a mismatched dSYM, and that dSYM must have
  // named nothing. `DSYM_MISSING` or any other refusal is not the same claim.
  const gateCode = report.negative?.gateCode ?? null;
  if (gateCode !== "DSYM_UUID_MISMATCH") {
    problems.push({
      code: "SYMBOLICATION_NEGATIVE_NOT_REFUSED",
      detail: `negative run reported gate ${JSON.stringify(gateCode)}, expected DSYM_UUID_MISMATCH`,
    });
  }
  const negativeNamed = report.negative?.namedFrames;
  if (negativeNamed !== 0) {
    problems.push({
      code: "SYMBOLICATION_NEGATIVE_NAMED",
      detail: `negative run named ${JSON.stringify(negativeNamed ?? null)} frame(s) under a mismatched dSYM`,
    });
  }

  // Tie the evidence to the archive that ships: the UUIDs the report symbolicated must be the ones
  // the dSYM archive manifest records for the shipped binary. Without this an operator could pair a
  // passing report from one build with the artifacts of another.
  const dsymManifest = readJson(join(releaseDir, "dsyms", "manifest.json"));
  const archived = archivedBinaryUuids(dsymManifest);
  if (archived.length === 0) {
    problems.push({
      code: "SYMBOLICATION_ARCHIVE_UNRECORDED",
      detail: `${join(releaseDir, "dsyms", "manifest.json")} records no UUID for the shipped ferryx binary`,
    });
  } else {
    const reported = Array.isArray(report.binary?.uuids) ? report.binary.uuids : [];
    const foreign = reported.filter((uuid) => !archived.includes(uuid));
    if (reported.length === 0 || foreign.length > 0) {
      problems.push({
        code: "SYMBOLICATION_UUID_NOT_ARCHIVED",
        detail:
          `evidence covers ${reported.join(", ") || "no UUID"}, but the archive records ${archived.join(", ")}; ` +
          "the report does not describe these artifacts",
      });
    }
  }

  // --- Check 1: the crash must be the harness's own forced SIGSEGV -----------------------------
  // `--ips <file>` lets an operator supply any historical crash report. That report could name a
  // named frame while proving nothing about this build, so a supplied report is refused unless the
  // report also proves a SIGSEGV this harness caused. The verdict field is not enough: only a
  // non-null `forcedCrash` with a real pid and SIGSEGV shows the process was signalled here.
  const forced = report.forcedCrash ?? null;
  if (!forced || typeof forced !== "object") {
    problems.push({
      code: "SYMBOLICATION_CRASH_NOT_FORCED",
      detail:
        "the report records no forced crash, so its crash report was not produced by this run; " +
        `a SIGSEGV must be forced against the release binary (drop --ips, or re-run the acceptance harness)`,
    });
  } else {
    if (forced.signal !== REQUIRED_CRASH_SIGNAL) {
      problems.push({
        code: "SYMBOLICATION_CRASH_SIGNAL",
        detail: `forcedCrash.signal is ${JSON.stringify(forced.signal ?? null)}, expected ${REQUIRED_CRASH_SIGNAL}`,
      });
    }
    if (!Number.isInteger(forced.pid) || forced.pid <= 0) {
      problems.push({
        code: "SYMBOLICATION_CRASH_PID_INVALID",
        detail: `forcedCrash.pid is ${JSON.stringify(forced.pid ?? null)}; a forced crash is identified by the pid that was signalled`,
      });
    }
  }

  // --- Check 2: the accepted binary must be the SHIPPED binary, by bytes -----------------------
  // A UUID match only proves the dSYM came from some link of that UUID. It does not prove the bytes
  // under acceptance are the bytes inside `artifacts/darwin/Ferryx.app.tar.gz`, so a debug build
  // symbolicated locally could satisfy the archive match while the shipped bytes stay unproven.
  //
  // The hash must be the one the harness captured at acceptance time, from `binary.sha256`.
  //
  // Re-hashing `binary.path` here was considered and rejected: this gate runs after the crash, on
  // an operator's filesystem, so the file at that path can have been replaced between the crash
  // and the gate. A digest taken then proves only what is on disk now, never what ran -- and the
  // whole point of this check is which bytes the crash report describes. A report without the
  // acceptance-time digest is refused instead, which keeps the gate honest about its evidence.
  const artifactPath = join(releaseDir, PACKAGED_APP_ARTIFACT);
  const acceptedSha =
    typeof report.binary?.sha256 === "string" && report.binary.sha256 ? report.binary.sha256 : null;
  if (!acceptedSha) {
    problems.push({
      code: "SYMBOLICATION_BINARY_UNHASHED",
      detail:
        "the report records no binary.sha256, so the accepted binary cannot be tied to the shipped " +
        "artifact: the digest must be captured at acceptance time (st_01a1e5ff owns that evidence)",
    });
  }
  const packaged = readMember({ artifactPath, memberPath: PACKAGED_APP_MEMBER });
  if (!packaged) {
    problems.push({
      code: "SYMBOLICATION_ARTIFACT_MISSING",
      detail: `${artifactPath} does not exist, so the symbolicated binary cannot be bound to what ships`,
    });
  } else if (packaged.error) {
    problems.push({ code: packaged.code ?? "SYMBOLICATION_ARTIFACT_UNREADABLE", detail: packaged.error });
  } else if (acceptedSha && packaged.sha256 !== acceptedSha) {
    problems.push({
      code: "SYMBOLICATION_BINARY_NOT_SHIPPED",
      detail:
        `the packaged ${PACKAGED_APP_MEMBER} hashes to ${packaged.sha256} but the accepted binary, per ` +
        `report.binary.sha256, hashes to ${acceptedSha}: different bytes, so this symbolication does not ` +
        "describe the shipped binary",
    });
  }

  return {
    ok: problems.length === 0,
    reportPath,
    report,
    archived,
    problems,
    blocking: problems,
    packagedSha256: packaged?.sha256 ?? null,
    forcedCrash: forced,
  };
}

/**
 * Builds the `dsyms/manifest.json` payload for one archive.
 *
 * `binaries` come from `produceDsymFromSlices` (one entry per shipped executable), `gates` from
 * `assertDsymGate` (one per gated binary). Paths are recorded relative to `runDir` when they live
 * under it, so the manifest stays readable after the run directory is moved or tarred.
 *
 * `keeper`, when present, records the generation the daemon installs under
 * `~/.ferryx/keeper/<version>/`, so a keeper frame can be paired with the crash report naming it.
 */
export function buildDsymArchiveManifest({
  runDir = null,
  binaries = [],
  gates = [],
  keeper = null,
  appVersion = null,
  runId = null,
  tag = null,
  createdAt = new Date().toISOString(),
}) {
  const relative = (value) => {
    if (!value || !runDir) return value ?? null;
    const prefix = `${String(runDir).replace(/[/\\]+$/, "")}/`;
    return String(value).startsWith(prefix) ? String(value).slice(prefix.length) : String(value);
  };
  return {
    schema: DSYM_ARCHIVE_SCHEMA,
    runId,
    tag,
    appVersion,
    createdAt,
    binaries: binaries.map((entry) => ({
      binary: entry.binary,
      uuids: [...(entry.uuids ?? [])],
      functions: entry.functions ?? null,
      dsym: relative(entry.dsym ?? entry.bundle),
    })),
    gate: gates.map((entry) => ({
      binary: entry.binaryPath ?? entry.binary,
      uuids: [...(entry.binaryUuids ?? entry.uuids ?? [])],
      dsyms: (entry.bundles ?? []).map((bundle) => relative(bundle.path ?? bundle)),
    })),
    ...(keeper ? { keeper } : {}),
  };
}

/**
 * Writes `dsyms/manifest.json` (creating the archive dir when needed) and returns its path.
 *
 * A dSYM archive that does not describe itself cannot be paired with symbolication evidence, so
 * every path that puts dSYMs into an archive calls this rather than leaving the file to chance.
 */
export function writeDsymArchiveManifest(dsymDir, manifest) {
  mkdirSync(dsymDir, { recursive: true });
  const manifestPath = join(dsymDir, "manifest.json");
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  return manifestPath;
}

function cell(value) {
  return value === null || value === undefined ? "-" : String(value);
}

/** Markdown report: what was proved, by which dSYM, and what blocks the release. */
export function renderSymbolicationReport(result) {
  const report = result.report ?? {};
  const lines = [
    "## Crash symbolication (Phase 0)",
    "",
    `Evidence: \`${result.reportPath}\`.`,
    "",
    "| Check | Result |",
    "| --- | --- |",
    `| Verdict | ${cell(report.verdict)} |`,
    `| Binary UUIDs | ${(report.binary?.uuids ?? []).join(", ") || "-"} |`,
    `| Forced crash | ${report.forcedCrash ? `${report.forcedCrash.signal ?? "?"} (pid ${report.forcedCrash.pid ?? "?"})` : "**none recorded**"} |`,
    `| Shipped bytes | ${result.packagedSha256 ? `sha256 ${result.packagedSha256.slice(0, 16)}...` : "**artifact unread**"} |`,
    `| Archived dSYM UUIDs | ${result.archived.join(", ") || "-"} |`,
    `| Named binary frames | ${cell(report.symbolicated?.namedFrames)} |`,
    `| Negative run: gate | ${cell(report.negative?.gateCode)} |`,
    `| Negative run: named frames | ${cell(report.negative?.namedFrames)} |`,
  ];
  if (result.blocking.length) {
    lines.push("", "Blocking findings:", "", ...result.blocking.map((p) => `- ${p.code}: ${p.detail}`));
  } else {
    lines.push(
      "",
      "A forced SIGSEGV of the release binary symbolicated to a named function through the archived",
      "dSYM, and a mismatched dSYM was refused and named nothing.",
    );
  }
  return `${lines.join("\n")}\n`;
}

/**
 * Runs the evidence gate and writes `symbolication/symbolication-report.md`. Throws
 * `SymbolicationGateError` carrying the first blocking code when the release must not be drafted.
 */
export function assertSymbolicationGate({ releaseDir }) {
  const result = evaluateSymbolicationEvidence({ releaseDir });
  const report = renderSymbolicationReport(result);
  const evidenceDir = join(releaseDir, SYMBOLICATION_EVIDENCE_DIR);
  const reportPath = join(evidenceDir, SYMBOLICATION_SUMMARY_NAME);
  if (existsSync(evidenceDir)) writeFileSync(reportPath, report);
  if (!result.ok) {
    throw new SymbolicationGateError(
      result.blocking[0].code,
      `crash symbolication gate refused the release (${result.blocking.map((p) => p.detail).join("; ")})`,
      result.blocking,
    );
  }
  return { result, report, reportPath };
}
