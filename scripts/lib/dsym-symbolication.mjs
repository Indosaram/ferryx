// Phase 0 acceptance primitives: prove a forced SIGSEGV of the RELEASE binary names a function
// through the archived dSYM, and that a mismatched dSYM names nothing instead of misleading.
//
// The gate (`release-dsym.mjs`) answers "is the archived dSYM the one built from these bytes?".
// This module answers the next question, which the plan's Phase 0 acceptance actually states:
// "does `symbolicatecrash` turn that binary's crash frames into a named function?". The two are
// separate because a UUID match says nothing about whether symbolication *succeeds* -- only a real
// crash report run through the real tool does.
//
// Everything here is pure: text in, verdict out. The runner
// (`scripts/qa/dsym-symbolication-acceptance.mjs`) supplies the file bytes and the forced crash, so
// these rules are testable without a build, a crash, or a Darwin host.
import { createHash } from "node:crypto";
import path from "node:path";
import { gunzipSync } from "node:zlib";

import { normalizeUuid } from "./release-dsym.mjs";

export const SYMBOLICATION_SCHEMA = "ferryx-dsym-symbolication/1";

/** Where the shipped binary sits inside a packaged `Ferryx.app`. */
export const APP_BINARY_MEMBER = "Ferryx.app/Contents/MacOS/ferryx";

/** A frame symbolicatecrash resolved to a function name. */
export const NAMED = "named";
/** A frame that is still an address/offset pair: exactly the `ferryx +<offset>` state Phase 0 ends. */
export const ANONYMOUS = "offset-only";

/** Typed refusal: `code` leads the message and the report's `verdict`. */
export class SymbolicationError extends Error {
  constructor(code, message, detail = {}) {
    super(`${code}: ${message}`);
    this.name = "SymbolicationError";
    this.code = code;
    this.detail = detail;
  }
}

function pushUnique(list, value) {
  if (value && !list.includes(value)) list.push(value);
}

/**
 * Build UUIDs a crash report names for images whose name/path matches `match`.
 *
 * A modern `.ips` is a one-line JSON header followed by a JSON body carrying `usedImages`; older
 * reports are one JSON document, and legacy text reports carry a `Binary Images:` section. All
 * three are read, because a report the OS wrote is not something this harness may reject for shape.
 */
export function crashImageUuids(text, match = /ferryx/i) {
  const source = String(text ?? "");
  const uuids = [];
  const payload = source.includes("\n") ? source.slice(source.indexOf("\n") + 1) : source;
  try {
    const document = JSON.parse(payload);
    for (const image of document.usedImages ?? []) {
      const name = image?.name ?? image?.path ?? "";
      if (typeof name !== "string" || !match.test(name)) continue;
      pushUnique(uuids, normalizeUuid(image?.uuid ?? ""));
    }
  } catch {
    // Not the JSON body (a legacy text report, or a truncated write); the text scan below runs.
  }
  if (uuids.length > 0) return uuids;
  for (const line of source.split("\n")) {
    if (!match.test(line)) continue;
    const bracketed = /<([0-9a-fA-F][0-9a-fA-F-]{31,35})>/.exec(line);
    if (!bracketed) continue;
    pushUnique(uuids, normalizeUuid(bracketed[1]));
  }
  return uuids;
}

/** Does any image the report names belong to the binary under acceptance? */
export function namesBinary(text, names = ["ferryx"]) {
  const source = String(text ?? "");
  return names.some((name) => new RegExp(`(^|[^\\w-])${name}($|[^\\w-])`, "i").test(source));
}

/**
 * The process identity a crash report records: the pids and executable paths it names.
 *
 * A modern `.ips` carries `pid`/`procPath` in its JSON body; a legacy text report carries
 * `Process: ferryx [43392]` and `Path: ...`. Both are read because binding a crash to the process
 * this run spawned must not depend on which shape ReportCrash happened to write.
 */
export function crashReportIdentity(text) {
  const source = String(text ?? "");
  const pids = [];
  const procPaths = [];
  const addPid = (value) => {
    const pid = Number(value);
    if (Number.isInteger(pid) && pid > 0 && !pids.includes(pid)) pids.push(pid);
  };
  const addPath = (value) => {
    const trimmed = String(value ?? "").trim();
    if (trimmed && !procPaths.includes(trimmed)) procPaths.push(trimmed);
  };
  const payload = source.includes("\n") ? source.slice(source.indexOf("\n") + 1) : source;
  try {
    const document = JSON.parse(payload);
    addPid(document?.pid);
    addPath(document?.procPath);
  } catch {
    // Not the JSON body; the text scan below runs.
  }
  for (const line of source.split("\n")) {
    const process = /^\s*Process:\s+(\S+)\s+\[(\d+)\]/.exec(line);
    if (process) addPid(process[2]);
    const pathLine = /^\s*Path:\s+(\/.*)$/.exec(line);
    if (pathLine) addPath(pathLine[1]);
  }
  return { pids, procPaths, uuids: crashImageUuids(source) };
}

/**
 * Build UUIDs (`LC_UUID`) of a Mach-O file read from its bytes.
 *
 * The shipped tarball's executable has to be identified without a temp file or a tool, so the
 * load commands are walked directly: thin 32/64-bit images and universal (fat) containers, each
 * slice's endianness taken from its own magic. A byte-level read is also what keeps this testable
 * off a Darwin host, which a `dwarfdump` call would not be.
 */
export function machOUuids(bytes) {
  const buffer = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes ?? []);
  const uuids = [];
  const pushUuid = (offset) => {
    if (offset + 16 > buffer.length) return;
    const hex = buffer.subarray(offset, offset + 16).toString("hex").toUpperCase();
    const uuid = `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
    if (!uuids.includes(uuid)) uuids.push(uuid);
  };
  const scanSlice = (base) => {
    if (base + 28 > buffer.length) return;
    const little = buffer.readUInt32LE(base);
    const big = buffer.readUInt32BE(base);
    const is64 = little === 0xfeedfacf || big === 0xfeedfacf;
    const is32 = little === 0xfeedface || big === 0xfeedface;
    if (!is64 && !is32) return;
    const littleEndian = little === 0xfeedfacf || little === 0xfeedface;
    const readU32 = (offset) => (littleEndian ? buffer.readUInt32LE(offset) : buffer.readUInt32BE(offset));
    const ncmds = readU32(base + 16);
    let offset = base + (is64 ? 32 : 28);
    for (let index = 0; index < ncmds; index += 1) {
      if (offset + 8 > buffer.length) return;
      const command = readU32(offset);
      const size = readU32(offset + 4);
      if (size < 8) return;
      if (command === 0x1b) pushUuid(offset + 8);
      offset += size;
    }
  };
  if (buffer.length >= 8) {
    const magic = buffer.readUInt32BE(0);
    if (magic === 0xcafebabe || magic === 0xcafebabf) {
      const count = buffer.readUInt32BE(4);
      const wide = magic === 0xcafebabf;
      const stride = wide ? 32 : 20;
      for (let index = 0; index < count; index += 1) {
        const entry = 8 + index * stride;
        if (entry + stride > buffer.length) break;
        const sliceOffset = wide ? Number(buffer.readBigUInt64BE(entry + 8)) : buffer.readUInt32BE(entry + 8);
        scanSlice(sliceOffset);
      }
      return uuids;
    }
  }
  scanSlice(0);
  return uuids;
}

/**
 * Parses `symbolicatecrash` output into frames, classifying each as [`NAMED`] or [`ANONYMOUS`].
 *
 * Two shapes appear in the tool's output: the classic column form
 * `0   ferryx   0x104a3f2a0   0x104000000 + 7401632` (anonymous) or
 * `0   ferryx   0x104a3f2a0   main + 32 (main.rs:10)` (named), and a body-only form where the
 * offset pair stands alone. `???` is what the tool prints when it has no symbol at all, so it must
 * count as anonymous: a gate that accepted `???` would pass the exact state Phase 0 exists to end.
 */
export function parseSymbolicatedFrames(text) {
  const frames = [];
  const seen = new Set();
  const record = (frame) => {
    const key = `${frame.index}|${frame.image}|${frame.address}|${frame.body}`;
    if (seen.has(key)) return;
    seen.add(key);
    frames.push(frame);
  };
  for (const line of String(text ?? "").split("\n")) {
    const columned = /^\s*(\d+)\s+(\S+)\s+(0x[0-9a-fA-F]+)\s+(.+?)\s*$/.exec(line);
    const bare = columned ? null : /^\s*(\d+)\s+(\S+)\s+(\+\s*\d+|\S+\s*\+\s*\d+.*)$/.exec(line);
    const match = columned ?? bare;
    if (!match) continue;
    const index = Number(match[1]);
    const image = match[2];
    const address = columned ? match[3] : null;
    const body = (columned ? match[4] : match[3]).trim();
    record({ index, image, address, body, kind: classifyBody(body) });
  }
  return frames;
}

/** [`ANONYMOUS`] for an address/offset pair or `???`; [`NAMED`] for anything else with an offset. */
export function classifyBody(body) {
  const value = String(body ?? "").trim();
  if (value === "" || value === "???") return ANONYMOUS;
  // `0x104000000 + 7401632` and `+ 7401632` carry no symbol, only a displacement.
  if (/^0x[0-9a-fA-F]+\s*\+\s*\d+$/.test(value) || /^\+\s*\d+$/.test(value)) return ANONYMOUS;
  // `main + 32 (main.rs:10)`, `foo::bar() + 8`, `<T as Trait>::f + 4`: a symbol, then its offset.
  if (/^.+?\s*\+\s*\d+\s*(\(.*\))?$/.test(value) && !/^0x[0-9a-fA-F]+$/.test(value)) return NAMED;
  // A demangled symbol that happens to end without an offset is still named.
  return /[A-Za-z_]/.test(value) ? NAMED : ANONYMOUS;
}

/** Frames whose image is one of `names` (the binary under acceptance, not a system library). */
export function binaryFrames(frames, names = ["ferryx"]) {
  const patterns = names.map((name) => new RegExp(`(^|[^\\w-])${name}($|[^\\w-])`, "i"));
  return frames.filter((frame) => patterns.some((pattern) => pattern.test(frame.image)));
}

/**
 * The acceptance decision for one symbolicated report.
 *
 * `ok` requires both halves: the crash report really is about this binary, and at least one of ITS
 * frames was resolved to a function name. A system-library frame resolved to a name proves nothing
 * about the dSYM under test, which is why the binary filter runs before the named check.
 */
export function evaluateSymbolication({ crashText, symbolicatedText, imageNames = ["ferryx"] }) {
  const crashUuids = crashImageUuids(crashText);
  const frames = parseSymbolicatedFrames(symbolicatedText);
  const framesOfBinary = binaryFrames(frames, imageNames);
  const named = framesOfBinary.filter((frame) => frame.kind === NAMED);
  const crashNamesBinary = namesBinary(crashText, imageNames);
  const checks = {
    crashReportNamesBinary: {
      ok: crashNamesBinary,
      detail: crashNamesBinary
        ? `crash report names ${imageNames.join("/")}`
        : `crash report names none of ${imageNames.join("/")}; this report cannot prove anything about the binary`,
    },
    namedFramePresent: {
      ok: named.length > 0,
      detail:
        named.length > 0
          ? `${named.length} named ${imageNames.join("/")} frame(s): ${named
              .slice(0, 3)
              .map((frame) => frame.body)
              .join(" | ")}`
          : `${framesOfBinary.length} ${imageNames.join("/")} frame(s), none named -- every one is an offset (${ANONYMOUS})`,
    },
  };
  return {
    crashUuids,
    frames,
    binaryFrames: framesOfBinary,
    namedFrames: named,
    anonymousFrames: framesOfBinary.filter((frame) => frame.kind === ANONYMOUS),
    checks,
    ok: Object.values(checks).every((check) => check.ok),
  };
}

/**
 * The negative decision: a dSYM that does not carry the binary's UUID must name nothing.
 *
 * `gateCode` is the typed refusal the release gate produced (it must be `DSYM_UUID_MISMATCH`), and
 * `namedFrames` must be empty. Both are required: a gate that merely failed to *find* the bundle, or
 * a symbolication that happened to resolve nothing for an unrelated reason, is not the proof.
 */
export function evaluateMismatchedDsym({ gateCode, symbolicatedText, imageNames = ["ferryx"] }) {
  const frames = parseSymbolicatedFrames(symbolicatedText);
  const named = binaryFrames(frames, imageNames).filter((frame) => frame.kind === NAMED);
  const checks = {
    gateRefusedMismatch: {
      ok: gateCode === "DSYM_UUID_MISMATCH",
      detail: `release gate refused with ${gateCode}`,
    },
    mismatchNamesNothing: {
      ok: named.length === 0,
      detail:
        named.length === 0
          ? "no named ferryx frame under the mismatched dSYM"
          : `mismatched dSYM produced ${named.length} named frame(s), which is impossible: ${named[0].body}`,
    },
  };
  return { frames, namedFrames: named, checks, ok: Object.values(checks).every((check) => check.ok) };
}

/** Hex SHA-256 of bytes; the identity that binds evidence to the bytes that ship. */
export function sha256Hex(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

/** Entries of an uncompressed tar: `[{name, bytes}]`, skipping directories and global headers. */
export function parseTarEntries(buffer) {
  const source = Buffer.isBuffer(buffer) ? buffer : Buffer.from(buffer ?? []);
  const entries = [];
  let offset = 0;
  let pendingName = null;
  while (offset + 512 <= source.length) {
    const header = source.subarray(offset, offset + 512);
    // Two zero blocks end the archive; a single one is enough to stop reading in practice.
    if (header.every((byte) => byte === 0)) break;
    const text = (start, length) => header.subarray(start, start + length).toString("utf8").replace(/\0.*$/s, "").trim();
    const name = text(0, 100);
    const prefix = text(345, 155);
    const rawSize = text(124, 12);
    const size = rawSize === "" ? 0 : parseInt(rawSize, 8);
    const type = text(156, 1) || "0";
    const dataStart = offset + 512;
    const data = source.subarray(dataStart, Math.min(dataStart + size, source.length));
    if (type === "L") {
      // GNU long name: the data is the real name of the next entry.
      pendingName = data.toString("utf8").replace(/\0.*$/s, "");
    } else if (type === "x") {
      const match = /^\d+ path=([\s\S]*)\n/.exec(data.toString("utf8"));
      if (match) pendingName = match[1];
    } else if (type !== "5" && type !== "g") {
      const fullName = pendingName ?? (prefix ? `${prefix}/${name}` : name);
      if (fullName && Number.isFinite(size)) entries.push({ name: fullName, bytes: data });
      pendingName = null;
    }
    const padded = Math.ceil(size / 512) * 512;
    offset = dataStart + padded;
  }
  return entries;
}

/**
 * Reads one member out of a `.tar.gz` artifact, e.g. `Ferryx.app/Contents/MacOS/ferryx`.
 *
 * The member is matched exactly (after normalizing a leading `./`), never by suffix: a tarball can
 * hold several executables, and symbolication evidence has to name the one that runs.
 */
export function readTarGzMember({ bytes, memberPath, gunzip = gunzipSync }) {
  const wanted = String(memberPath ?? "").replace(/^\.\//, "");
  if (!wanted) return null;
  let tar;
  try {
    tar = gunzip(Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes ?? []));
  } catch {
    return null;
  }
  const entries = parseTarEntries(tar);
  const exact = entries.find((entry) => entry.name.replace(/^\.\//, "") === wanted);
  if (exact) return { name: wanted, bytes: exact.bytes, sha256: sha256Hex(exact.bytes) };
  return null;
}

/**
 * The dSYM bundles that actually carry a crash image's UUID, in the order they were archived.
 *
 * `symbolicatecrash` is handed these exact paths, so recording them is what lets an operator (or a
 * later audit) re-run the same command instead of guessing which bundle in a directory was used.
 */
export function resolveSymbolicationBundles({ bundles = [], uuids = [] } = {}) {
  const wanted = new Set(uuids);
  const matched = bundles.filter((bundle) => (bundle?.uuids ?? []).some((uuid) => wanted.has(uuid)));
  const covered = new Set(matched.flatMap((bundle) => bundle.uuids ?? []));
  return {
    paths: matched.map((bundle) => bundle.path),
    bundles: matched,
    missing: [...wanted].filter((uuid) => !covered.has(uuid)),
  };
}

/**
 * The forced-crash record written into the evidence report.
 *
 * Every field is a thing this run observed: the pid it signalled, the exact executable it spawned,
 * the token that proved the daemon was serving, the report path the OS wrote afterwards, and the
 * identity that report states. It is built here (not in the runner) so the same shape is produced
 * for tests as for a real crash.
 */
export function buildForcedCrashRecord({ outcome = null, crashText = "", reportPath = null, reportWrittenAfterCrash = false } = {}) {
  if (!outcome) return null;
  const identity = crashReportIdentity(crashText);
  // The report's own header timestamp is the only independent statement of WHEN it was written;
  // comparing it with the spawn time is what proves the operator did not hand in an old report.
  const headerTimestamp = crashReportTimestamp(crashText);
  return {
    way: "forced-segv",
    pid: outcome.pid ?? null,
    signal: outcome.signal ?? null,
    spawnedBinary: outcome.spawnedBinary ? path.resolve(outcome.spawnedBinary) : null,
    // Captured BEFORE spawn and re-verified after: the two halves of the bytes binding.
    spawnedSha256: outcome.spawnedSha256 ?? null,
    sha256UnchangedAfterCrash: outcome.sha256UnchangedAfterCrash === true,
    binaryUuids: outcome.binaryUuids ?? [],
    spawnedAt: outcome.spawnedAt ?? null,
    readyToken: outcome.readyToken ?? null,
    readyAt: outcome.readyAt ?? null,
    reportedAt: new Date().toISOString(),
    reportPath: reportPath ? path.resolve(reportPath) : null,
    reportPids: identity.pids,
    reportProcPaths: identity.procPaths,
    reportUuids: identity.uuids,
    reportTimestamp: headerTimestamp?.raw ?? null,
    reportWrittenAt: headerTimestamp?.iso ?? null,
    reportWrittenAfterCrash: reportWrittenAfterCrash === true,
  };
}

/**
 * The timestamp a crash report states for itself, as `{raw, iso}`.
 *
 * A modern `.ips` header carries `"timestamp":"2026-11-16 04:00:00.00 +0900"`; a legacy text
 * report carries `Date/Time: 2026-11-16 04:00:00.000 +0900`. Both are normalized to ISO so the
 * ordering against the spawn time is a plain comparison and not a string-luck test.
 */
export function crashReportTimestamp(text) {
  const source = String(text ?? "");
  const raw =
    /"timestamp"\s*:\s*"([^"]+)"/.exec(source)?.[1] ??
    /^\s*Date\/Time:\s+(.+?)\s*$/m.exec(source)?.[1] ??
    null;
  if (!raw) return null;
  // `+0900` must become `+09:00` before Date.parse reads the offset reliably.
  const normalized = raw.replace(/([+-])(\d{2})(\d{2})$/, "$1$2:$3").replace(/\s+/, " ");
  const parsed = Date.parse(normalized);
  return { raw, iso: Number.isFinite(parsed) ? new Date(parsed).toISOString() : null };
}

/**
 * The provenance decision: the crash must be one THIS run forced, not a report handed in later.
 *
 * The release gate reads `checks` and refuses a release when any named check is not `ok`, so this
 * check is what stops `--ips some-old.ips` from reaching PASS. Every clause is an observation the
 * harness made: the child was spawned by the harness (pid), it announced readiness before the
 * signal, the signal was SIGSEGV, and the report is one that did not exist when the run began.
 * `readyToken` proves the crash came after the daemon was actually serving; signalling a booting
 * process would prove a crash site that is not the one users hit.
 */
export function evaluateForcedCrashProvenance({ forced = null, acceptedBinary = null } = {}) {
  const problems = [];
  if (!forced) {
    problems.push(
      "no forced crash was recorded: the crash came from --ips, so nothing proves the release binary was made to segfault",
    );
  } else {
    if (forced.signal !== "SIGSEGV") {
      problems.push(`the process left with ${forced.signal ?? "no signal"} instead of SIGSEGV`);
    }
    if (!Number.isInteger(forced.pid) || forced.pid <= 0) {
      problems.push(`the crashed pid was not recorded (${JSON.stringify(forced.pid ?? null)})`);
    }
    if (!forced.readyToken) {
      problems.push("no readiness token was observed before the signal, so the crash site is not the serving daemon's");
    }
    if (forced.reportWrittenAfterCrash !== true) {
      problems.push("the crash report was not proven to be written after this run started");
    }
    if (acceptedBinary && forced.spawnedBinary && path.resolve(forced.spawnedBinary) !== path.resolve(acceptedBinary)) {
      problems.push(
        `the crashed binary ${forced.spawnedBinary} is not the binary under acceptance ${path.resolve(acceptedBinary)}`,
      );
    }
    // Binding the beat: the pid that was signalled must be the pid the crash report names.
    // Without this a report written by any other process could be analysed and still pass every
    // symbolication check, which is the whole gap `--ips` exploited.
    if (Array.isArray(forced.reportPids) && Number.isInteger(forced.pid) && !forced.reportPids.includes(forced.pid)) {
      problems.push(
        `the crash report names pid(s) ${forced.reportPids.join(", ") || "none"}, but the process this run signalled was pid ${forced.pid}`,
      );
    }
    // The report must also name the executable that was spawned, not merely a same-named image.
    if (Array.isArray(forced.reportProcPaths) && forced.spawnedBinary) {
      const spawned = path.resolve(forced.spawnedBinary);
      const named = forced.reportProcPaths.map((value) => path.resolve(value));
      if (named.length > 0 && !named.includes(spawned)) {
        problems.push(
          `the crash report names executable ${named.join(", ")}, not the binary this run spawned (${spawned})`,
        );
      }
    }
    // The digest must be the one captured BEFORE the executable ran. A hash taken from the path
    // afterwards describes whatever file is there later, which is precisely the weakness of a
    // post-hoc `--ips` workflow; only a pre-execution capture binds evidence to the bytes that died.
    if (!forced.spawnedSha256) {
      problems.push(
        "the digest of the spawned executable was not captured before the crash, so the symbolicated bytes are not bound to the process that died",
      );
    }
    if (forced.sha256UnchangedAfterCrash !== true) {
      problems.push(
        "the spawned executable was not shown to hash the same before and after the crash, so the evidence may describe different bytes than the ones that ran",
      );
    }
    // Ordering: the report cannot predate the spawn. A report older than the process proves the
    // operator handed in a historical crash.
    const spawnedAt = Date.parse(forced.spawnedAt ?? "");
    const reportAt = Date.parse(forced.reportWrittenAt ?? "");
    if (!Number.isFinite(spawnedAt)) {
      problems.push("the spawn time was not recorded, so the crash report cannot be ordered against it");
    } else if (Number.isFinite(reportAt) && reportAt < spawnedAt) {
      problems.push(
        `the crash report is dated ${forced.reportWrittenAt}, before this run spawned the binary at ${forced.spawnedAt}`,
      );
    }
    // The report must name one of the BINARY's build UUIDs, read before the crash: a report for a
    // different build of the same name is not this crash.
    const binaryUuids = Array.isArray(forced.binaryUuids) ? forced.binaryUuids : [];
    const reportUuids = Array.isArray(forced.reportUuids) ? forced.reportUuids : [];
    if (binaryUuids.length === 0) {
      problems.push("the spawned executable's build UUID was not read before the crash");
    } else if (reportUuids.length > 0 && !reportUuids.some((uuid) => binaryUuids.includes(uuid))) {
      problems.push(
        `the crash report names ${reportUuids.join(", ")}, none of which is the spawned binary's ${binaryUuids.join(", ")}`,
      );
    }
  }
  return {
    ok: problems.length === 0,
    detail:
      problems.length === 0
        ? `forced SIGSEGV (pid ${forced.pid}) of ${forced.spawnedBinary ?? "the release binary"} (sha256 ${forced.spawnedSha256}, unchanged after the crash) after ${forced.readyToken}; the crash report names pid ${forced.reportPids?.join(", ") ?? forced.pid}`
        : problems.join("; "),
  };
}

/**
 * The identity of the executable this run crashed, as bytes plus the two observations around it.
 *
 * `before` is read BEFORE the process runs and `after` once it has exited. Both are needed: the
 * pre-image is the only description of the bytes that actually died (a hash taken from the path
 * later describes whatever file is there then), and the post-image is what proves the executable
 * was not replaced underneath the run. The record is built here so the runner and its tests emit
 * the same shape, and so `binary.sha256` in the evidence report is always the pre-crash digest.
 */
export function buildBinaryIdentity({ path: binaryPath = null, before, after = null } = {}) {
  const preImage = Buffer.isBuffer(before) ? before : Buffer.from(before ?? []);
  const postImage = after == null ? null : Buffer.isBuffer(after) ? after : Buffer.from(after);
  const sha256 = sha256Hex(preImage);
  const sha256AfterCrash = postImage ? sha256Hex(postImage) : null;
  return {
    path: binaryPath ? path.resolve(binaryPath) : null,
    sha256,
    sha256AfterCrash,
    bytes: preImage.length,
    uuids: machOUuids(preImage),
    capturedBeforeCrash: true,
    // Named to match `evaluatePackagedBinaryIdentity`, which requires this exact flag: one shape
    // for the accepted binary whether it is read here or from a recorded identity.
    verifiedUnchangedAfterCrash: postImage ? sha256AfterCrash === sha256 : false,
  };
}

/**
 * The identity decision: `binary.sha256` is the digest of the process that crashed, and it is the
 * same bytes before and after.
 *
 * The release gate reads `binary.sha256` and compares it with the executable inside the shipped
 * tarball. That comparison is only meaningful when the digest came from the pre-execution read and
 * the file is unchanged afterwards -- otherwise it describes bytes that never ran, or a file that
 * was swapped mid-run. The path is checked too, so evidence for a different executable cannot ride
 * on a matching digest, and the digest must agree with the forced crash's own `spawnedSha256`.
 */
export function evaluateBinaryIdentity({ binary = null, forced = null } = {}) {
  const problems = [];
  const sha256 = binary?.sha256 ?? null;
  if (!sha256) {
    problems.push(
      "the report records no binary.sha256, so the release gate cannot bind the symbolicated bytes to the shipped artifact",
    );
  }
  if (binary && binary.capturedBeforeCrash !== true) {
    problems.push("binary.sha256 was not captured before the process ran, so it does not describe the bytes that crashed");
  }
  if (sha256 && binary?.verifiedUnchangedAfterCrash !== true) {
    problems.push(
      `binary.sha256 (${sha256}) was not shown to be unchanged after the crash, so the executable may have been replaced mid-run`,
    );
  }
  if (!forced) {
    problems.push("no forced crash was recorded, so binary.sha256 is not tied to a process this run signalled");
  } else {
    if (sha256 && forced.spawnedSha256 && forced.spawnedSha256 !== sha256) {
      problems.push(
        `binary.sha256 is ${sha256} but the crashed process was spawned from ${forced.spawnedSha256}: the report does not describe the bytes that died`,
      );
    }
    if (binary?.path && forced.spawnedBinary && path.resolve(binary.path) !== path.resolve(forced.spawnedBinary)) {
      problems.push(
        `binary.path is ${path.resolve(binary.path)} but the crashed process ran ${path.resolve(forced.spawnedBinary)}`,
      );
    }
  }
  return {
    ok: problems.length === 0,
    detail:
      problems.length === 0
        ? `binary.sha256 ${sha256} captured before the crash of pid ${forced.pid}, unchanged after it, at ${binary.path}`
        : problems.join("; "),
  };
}

/**
 * The packaging decision: the binary that was symbolicated IS the binary inside the shipped artifact.
 *
 * A UUID match proves the dSYM came from some link of that UUID; it does not prove the bytes under
 * acceptance are the bytes in `artifacts/darwin/Ferryx.app.tar.gz`. Hashes close that: the copy
 * extracted from the artifact must hash to the accepted binary, and carry the same build UUIDs.
 * Without this an operator can symbolicate a locally built debug binary and satisfy the archive
 * match while the shipped bytes stay unproven -- the exact gap this check exists to close.
 */
export function evaluatePackagedBinaryIdentity({ accepted = null, packaged = null } = {}) {
  const problems = [];
  const acceptedSha = accepted?.sha256 ?? null;
  const packagedSha = packaged?.sha256 ?? null;
  // The accepted digest must have been taken while the binary was in hand, before it ran, and
  // re-checked after. A hash computed from the path afterwards is a weaker claim: the file can be
  // replaced in between, so it cannot prove which bytes the crash report describes.
  if (accepted && accepted.capturedBeforeCrash !== true) {
    problems.push(
      "the accepted binary's digest was not captured before the crash; a hash taken afterwards cannot prove which bytes ran",
    );
  }
  if (accepted && accepted.verifiedUnchangedAfterCrash !== true) {
    problems.push(
      "the accepted binary's digest was not re-checked after the crash, so a replaced binary would go unnoticed",
    );
  }
  // A digest-only or UUID-only bind is not enough on its own: the same bytes can ship under a
  // rebuilt signature, and a UUID identifies a link, not the archive member. Both are required.
  if (!packaged?.artifact?.path) {
    problems.push(
      "no shipped artifact was recorded, so the symbolicated binary is not bound to anything that was published",
    );
  }
  if (!packaged?.artifact?.memberPath) {
    problems.push(
      "the shipped artifact does not record which member is the executable, so the identity cannot be re-checked later",
    );
  }
  if (!acceptedSha) problems.push(`the binary under acceptance could not be hashed (${accepted?.path ?? "no path"})`);
  if (!packaged?.sha256) {
    problems.push(
      packaged?.artifact?.path
        ? `no bytes could be read for ${packaged?.artifact?.memberPath ?? "the packaged member"} inside ${packaged.artifact.path}`
        : "no packaged artifact was read, so the accepted binary is not shown to be the shipped binary",
    );
  } else if (acceptedSha && packagedSha !== acceptedSha) {
    problems.push(
      `the packaged ${packaged?.artifact?.memberPath ?? "member"} hashes to ${packagedSha} but the accepted binary hashes to ${acceptedSha}: different bytes, so this symbolication does not describe the shipped binary`,
    );
  }
  const acceptedUuids = accepted?.uuids ?? [];
  const packagedUuids = packaged?.uuids ?? [];
  if (acceptedUuids.length === 0) problems.push("the binary under acceptance has no readable build UUID");
  if (packaged?.sha256 && packagedUuids.length === 0) {
    problems.push("the packaged copy has no readable build UUID, so its identity cannot be checked");
  }
  const missing = acceptedUuids.filter((uuid) => !packagedUuids.includes(uuid));
  if (missing.length > 0) {
    problems.push(`the packaged copy does not carry ${missing.join(", ")}, which the accepted binary carries`);
  }
  return {
    ok: problems.length === 0,
    detail:
      problems.length === 0
        ? `packaged ${packaged.artifact.memberPath} inside ${packaged.artifact.path} is byte-identical to the accepted binary (sha256 ${acceptedSha}) and carries ${acceptedUuids.join(", ")}`
        : problems.join("; "),
  };
}

/** One-line verdict for the operator, and the whole report in `--json` mode. */
export function renderSymbolicationSummary(report) {
  const lines = [
    `verdict: ${report.verdict}`,
    `binary: ${report.binary.path}`,
    `binary sha256: ${report.binary.sha256 ?? "unrecorded"}${
      report.binary.verifiedUnchangedAfterCrash === true ? " (unchanged after the crash)" : ""
    }`,
    `binary UUIDs: ${report.binary.uuids.join(", ") || "none readable"}`,
    `dSYM archive: ${report.dsymArchive.dir} (${report.dsymArchive.uuids.join(", ") || "no UUIDs"})`,
    `crash report: ${report.crash.path ?? "not forced"} (${report.crash.uuids.join(", ") || "no ferryx UUID"})`,
    `named frames: ${report.symbolicated.namedFrames}`,
    `negative run: gate ${report.negative.gateCode}, ${report.negative.namedFrames} named frame(s)`,
    `provenance: ${report.forcedCrash?.way ?? "unrecorded"}${
      report.forcedCrash?.pid
        ? ` (pid ${report.forcedCrash.pid}, ${report.forcedCrash.signal}, ${report.forcedCrash.spawnedBinary ?? "binary unrecorded"})`
        : ""
    }`,
    `crash report pid(s): ${report.forcedCrash?.reportPids?.join(", ") ?? "unrecorded"}`,
    `packaged binary: ${report.packagedBinary?.artifact?.path ?? "none"}${
      report.packagedBinary?.artifact?.memberPath ? ` -> ${report.packagedBinary.artifact.memberPath}` : ""
    }${report.packagedBinary?.sha256 ? ` (sha256 ${report.packagedBinary.sha256})` : ""}`,
    `dSYM bundles symbolicated: ${report.dsymTargets?.paths?.join(", ") || "none"}`,
    `evidence: ${report.reportDir}`,
  ];
  for (const [name, check] of Object.entries(report.checks)) {
    lines.push(`  ${check.ok ? "PASS" : "FAIL"} ${name}: ${check.detail}`);
  }
  return lines.join("\n");
}
