#!/usr/bin/env node
/**
 * Canonical local installer for Ferryx.app on macOS.
 *
 * Fails closed unless the bundle is a Developer-ID-signed, notarized, and
 * stapled macOS app. Plain `cargo tauri build` output is ad-hoc/linker
 * signed and must never be copied into /Applications directly; route every
 * local install through this script so unsigned bundles cannot reach the
 * Applications folder.
 *
 * Session safety: the live bundle is never deleted or re-signed. If any
 * process is executing the installed bundle, the existing bundle is renamed
 * (inode-preserving) into /Applications/.ferryx-previous-<ts>/ so running
 * daemons keep their PTY sessions, and the caller is reminded to trigger the
 * UDS `upgradeBinary` handover before launching the GUI.
 */
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, renameSync } from "node:fs";
import path from "node:path";

export const REQUIRED_TEAM_ID = "5DUM8WPB4C";
export const UNSIGNED_OVERRIDE_ENV = "FERRYX_ALLOW_UNSIGNED_INSTALL";
/**
 * Escape hatch for deliberately reinstalling the SAME version (e.g. re-verifying one
 * build). Normal installs must advance the version; see `evaluateVersionProgression`.
 */
export const VERSION_REUSE_OVERRIDE_ENV = "FERRYX_ALLOW_VERSION_REUSE_INSTALL";
export const DEFAULT_DEST = "/Applications/Ferryx.app";

/**
 * Pure evaluation of macOS signing evidence. Unit-tested; no side effects.
 */
export function evaluateNotarizationEvidence({
  codesignDv = "",
  spctl = "",
  staplerExit = 1,
  requireTeamId = REQUIRED_TEAM_ID,
} = {}) {
  const failures = [];
  if (/(^|\n)Signature=adhoc\s*(\n|$)/.test(codesignDv)) {
    failures.push("bundle is ad-hoc/linker signed (dev build); it can never pass Gatekeeper");
  }
  if (!codesignDv.includes("Authority=Developer ID Application:")) {
    failures.push("codesign -dv shows no 'Developer ID Application' authority");
  }
  if (requireTeamId && !codesignDv.includes(`TeamIdentifier=${requireTeamId}`)) {
    failures.push(`codesign TeamIdentifier is not ${requireTeamId}`);
  }
  if (!(spctl.includes("accepted") && spctl.includes("source=Notarized Developer ID"))) {
    failures.push(
      `Gatekeeper verdict is not "Notarized Developer ID" (spctl: ${spctl.trim() || "<no output>"})`,
    );
  }
  if (staplerExit !== 0) {
    failures.push("xcrun stapler validate did not pass (no notarization ticket)");
  }
  return { ok: failures.length === 0, failures };
}

/**
 * Numeric comparator for the dotted versions this repo ships (`2026.924.1`). Returns
 * `null` when either side is not a plain dotted-numeric version, so the caller can fall
 * back to an equality-only check instead of guessing an order.
 */
export function compareVersions(left, right) {
  const parse = (value) => {
    if (typeof value !== "string") return null;
    const parts = value.trim().replace(/^v/, "").split(".");
    if (parts.length < 2) return null;
    const numbers = [];
    for (const part of parts) {
      if (!/^\d+$/.test(part)) return null;
      numbers.push(Number(part));
    }
    return numbers;
  };
  const a = parse(left);
  const b = parse(right);
  if (!a || !b) return null;
  for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
    const l = a[i] ?? 0;
    const r = b[i] ?? 0;
    if (l !== r) return l < r ? -1 : 1;
  }
  return 0;
}

/**
 * Pure version-progression decision for a local install.
 *
 * A local install must ADVANCE the app version. Replacing the installed bundle with one
 * carrying the same `CFBundleShortVersionString` leaves the daemon's upgrade detection
 * with nothing but binary mtimes to compare, which is how a same-version replacement
 * reached /Applications on 2026-09-24 and forced an mtime-based handover instead of a
 * version-based one.
 */
export function evaluateVersionProgression({
  incomingVersion = "",
  installedVersion = "",
  reuseOverride = false,
} = {}) {
  const failures = [];
  if (reuseOverride) return { ok: true, failures };
  if (!incomingVersion) {
    failures.push(
      "incoming bundle has no CFBundleShortVersionString, so the version cannot be proven to advance",
    );
    return { ok: false, failures };
  }
  if (!installedVersion) return { ok: true, failures };
  const order = compareVersions(incomingVersion, installedVersion);
  if (order === 0) {
    failures.push(
      `incoming bundle version ${incomingVersion} equals the installed version; ` +
        "bump the version before installing so upgrade detection does not fall back to mtimes",
    );
  } else if (order !== null && order < 0) {
    failures.push(
      `incoming bundle version ${incomingVersion} is older than the installed version ${installedVersion}`,
    );
  }
  return { ok: failures.length === 0, failures };
}

/** Reads a bundle's marketing version from its Info.plist. */
export function bundleVersionCommand(bundle) {
  return [
    "/usr/libexec/PlistBuddy",
    ["-c", "Print :CFBundleShortVersionString", path.join(bundle, "Contents", "Info.plist")],
  ];
}

/**
 * Pure install-strategy decision. The installed bundle is ALWAYS moved to a
 * timestamped backup directory via rename (inode-preserving) - it is never
 * deleted or re-signed while processes may be executing it.
 */
export function decideInstallStrategy({ liveExecutors = [], unsignedOverride = false } = {}) {
  const reasons = [];
  if (liveExecutors.length > 0) {
    reasons.push(
      `${liveExecutors.length} live process(es) execute the installed bundle ` +
        `(pids ${liveExecutors.join(", ")}); rename the bundle instead of deleting it`,
    );
  }
  if (unsignedOverride) {
    reasons.push("unsigned install override active; new bundle is NOT verified");
  }
  return { action: "backup-then-replace", reasons };
}

export function timestampedBackupDir(now = new Date()) {
  const stamp = now
    .toISOString()
    .replaceAll("-", "")
    .replaceAll(":", "")
    .replace(/\..+$/, "")
    .replace("T", "-");
  return `/Applications/.ferryx-previous-${stamp}`;
}

export function liveExecutorCommand(dest = DEFAULT_DEST) {
  return [
    "/bin/sh",
    "-c",
    `pgrep -f ${quoteSh(path.join(dest, "Contents/MacOS/ferryx"))} | while read -r pid; do ` +
      `path=$(lsof -p "$pid" 2>/dev/null | awk '$4=="txt"{print $NF; exit}'); ` +
      `case "$path" in ${quoteSh(dest)}/*) echo "$pid";; esac; done`,
  ];
}

function quoteSh(value) {
  return `'${String(value).replaceAll("'", `'\\''`)}'`;
}

function run(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8" });
  return {
    exit: result.status === null ? -1 : result.status,
    out: `${result.stdout ?? ""}${result.stderr ?? ""}`,
  };
}

function parseArgs(argv) {
  const parsed = { bundle: null, dest: DEFAULT_DEST, allowUnsigned: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--bundle") parsed.bundle = argv[++i] ?? null;
    else if (arg === "--dest") parsed.dest = argv[++i] ?? DEFAULT_DEST;
    else if (arg === "--allow-unsigned") parsed.allowUnsigned = true;
    else {
      console.error(`Unknown argument: ${arg}`);
      process.exit(2);
    }
  }
  return parsed;
}

export function main(argv = process.argv.slice(2), { exec = run, env = process.env } = {}) {
  const options = parseArgs(argv);
  if (!options.bundle || !existsSync(options.bundle)) {
    console.error("Usage: node scripts/install-macos-app.mjs --bundle <path/to/Ferryx.app>");
    process.exit(2);
  }

  const codesign = exec("codesign", ["-dv", "--verbose=2", options.bundle]);
  const verify = exec("codesign", ["--verify", "--deep", "--strict", options.bundle]);
  const spctl = exec("spctl", ["-a", "-t", "exec", "-vvv", options.bundle]);
  const stapler = exec("xcrun", ["stapler", "validate", options.bundle]);

  const evidence = evaluateNotarizationEvidence({
    codesignDv: codesign.out,
    spctl: spctl.out,
    staplerExit: stapler.exit,
  });
  if (verify.exit !== 0) {
    evidence.failures.push(`codesign --verify --deep --strict failed: ${verify.out.trim()}`);
  }
  const unsignedOverride =
    options.allowUnsigned && env[UNSIGNED_OVERRIDE_ENV] === "1";

  if (!evidence.ok && !unsignedOverride) {
    console.error("Refusing to install: the bundle is not a notarized Developer ID app.");
    for (const failure of evidence.failures) console.error(`  - ${failure}`);
    console.error(
      `Build with APPLE_SIGNING_IDENTITY, notarize with xcrun notarytool, and staple, ` +
        `or set ${UNSIGNED_OVERRIDE_ENV}=1 plus --allow-unsigned to override explicitly.`,
    );
    process.exit(1);
  }
  if (unsignedOverride) {
    console.error(
      `WARNING: installing an unsigned/ad-hoc bundle by explicit override ` +
        `(${UNSIGNED_OVERRIDE_ENV}=1 --allow-unsigned). This build will trigger ` +
        "Gatekeeper/TCC re-prompts on every rebuild. Record why this is necessary.",
    );
  }

  // A local install must advance the app version. Replacing the bundle with the same
  // version leaves daemon upgrade detection with only binary mtimes to compare.
  const incomingVersionResult = exec(...bundleVersionCommand(options.bundle));
  const incomingVersion = incomingVersionResult.exit === 0 ? incomingVersionResult.out.trim() : "";
  const installedVersionResult = existsSync(options.dest)
    ? exec(...bundleVersionCommand(options.dest))
    : { exit: 1, out: "" };
  const installedVersion = installedVersionResult.exit === 0 ? installedVersionResult.out.trim() : "";
  const reuseOverride = env[VERSION_REUSE_OVERRIDE_ENV] === "1";
  const progression = evaluateVersionProgression({
    incomingVersion,
    installedVersion,
    reuseOverride,
  });
  if (!progression.ok) {
    console.error("Refusing to install: the bundle does not advance the app version.");
    for (const failure of progression.failures) console.error(`  - ${failure}`);
    console.error(
      "Bump the version in src-tauri/Cargo.toml and src-tauri/tauri.conf.json before installing, " +
        `or set ${VERSION_REUSE_OVERRIDE_ENV}=1 to reuse the version deliberately.`,
    );
    process.exit(1);
  }
  if (reuseOverride) {
    console.error(
      `WARNING: version-reuse override active (${VERSION_REUSE_OVERRIDE_ENV}=1); installing ` +
        `${incomingVersion} over ${installedVersion}. Upgrade detection will fall back to mtimes.`,
    );
  }
  console.log(`Version progression: ${installedVersion || "<none>"} -> ${incomingVersion}`);

  const liveCmd = liveExecutorCommand(options.dest);
  const liveExecutors = exec(liveCmd[0], liveCmd.slice(1)).out
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const strategy = decideInstallStrategy({ liveExecutors, unsignedOverride });

  const backupDir = timestampedBackupDir();
  if (existsSync(options.dest)) {
    mkdirSync(backupDir, { recursive: true });
    const backupPath = path.join(backupDir, path.basename(options.dest));
    renameSync(options.dest, backupPath);
    console.log(`Moved existing bundle to ${backupPath} (inode preserved; running daemons unaffected).`);
  }
  cpSync(options.bundle, options.dest, { recursive: true });
  console.log(`Installed ${options.bundle} -> ${options.dest}`);

  if (!unsignedOverride) {
    const installed = exec("codesign", ["-dv", "--verbose=2", options.dest]);
    const recheck = evaluateNotarizationEvidence({
      codesignDv: installed.out,
      spctl: exec("spctl", ["-a", "-t", "exec", "-vvv", options.dest]).out,
      staplerExit: exec("xcrun", ["stapler", "validate", options.dest]).exit,
    });
    if (!recheck.ok) {
      console.error("Installed bundle failed re-verification; investigate before launching:");
      for (const failure of recheck.failures) console.error(`  - ${failure}`);
      process.exit(1);
    }
  }

  console.log(strategy.reasons.length ? `Notes:\n  - ${strategy.reasons.join("\n  - ")}` : "");
  console.log(
    "Next steps:\n" +
      `  1. Trigger handover over UDS: {"type":"upgradeBinary","newBinaryPath":"${options.dest}/Contents/MacOS/ferryx"}\n` +
      "  2. Then launch the GUI (open -a Ferryx). Never kill draining daemons; they retire when their sessions end.",
  );
}

if (process.argv[1] && import.meta.url.endsWith(path.basename(process.argv[1]))) {
  main();
}
