/**
 * Packaging-time assembly of the macOS daemon helper bundle.
 *
 * The daemon must not run from the GUI's own executable inside the app install
 * (replacing or moving the app would change the running daemon's binary). The
 * release therefore ships a complete, signed helper bundle at
 *   <App>.app/Contents/Helpers/Ferryx Daemon.app
 * whose executable is a copy of the app's own binary. At runtime the daemon
 * runtime stager copies this bundle byte-for-byte into a generation-keyed
 * directory and verifies its signature; nothing at runtime builds or edits a
 * bundle.
 *
 * Signing order is inside-out: assembleDaemonHelper signs the helper bundle,
 * then finalizeMacosBundle (scripts/lib/release-platforms.mjs) re-signs loose
 * Mach-O files, every nested bundle under Contents/Helpers, and finally the
 * outer app, passing the outer app's recorded entitlements to both bundle
 * signatures so the outer app keeps exactly the entitlements it was built with.
 */
import { spawnSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

export const HELPER_BUNDLE_NAME = "Ferryx Daemon.app";
export const HELPER_BUNDLE_ID = "com.ferryx.app.daemon";
export const HELPER_EXECUTABLE = "ferryx";

/**
 * Runs a command and returns captured stdio; throws on a nonzero exit unless
 * `allowFailure` is set. Same contract as release-platforms macExec.
 */
export function helperExec(command, args, { allowFailure = false } = {}) {
  const res = spawnSync(command, args, { encoding: "utf8" });
  if (res.error) throw res.error;
  const stdout = res.stdout || "";
  const stderr = res.stderr || "";
  if (res.status !== 0 && !allowFailure) {
    throw new Error(
      `Command failed (exit ${res.status}): ${command} ${args.join(" ")}\n${(stderr || stdout).trim()}`,
    );
  }
  return { status: res.status ?? 0, stdout, stderr };
}

export function helperBundlePath(appPath) {
  return join(appPath, "Contents", "Helpers", HELPER_BUNDLE_NAME);
}

/**
 * Codesign arguments shared by every signature this pipeline makes. The ad-hoc
 * identity "-" (debug/QA) gets no secure timestamp: there is no Developer ID
 * signature for a timestamp to attest.
 */
export function codesignArgs(identity, entitlementsPath = null) {
  const args = ["--force", "--options", "runtime"];
  if (identity !== "-") args.push("--timestamp");
  args.push("--sign", identity);
  if (entitlementsPath) args.push("--entitlements", entitlementsPath);
  return args;
}

/**
 * Reads the entitlements embedded in a code signature as XML, or "" when the
 * signature carries none. An unsigned target also has none; any other failure
 * is an error, never silently treated as "no entitlements".
 */
export function readEntitlementsXml(target, exec = helperExec) {
  const res = exec("codesign", ["-d", "--entitlements", ":-", target], { allowFailure: true });
  if (res.status !== 0) {
    if (`${res.stdout}\n${res.stderr}`.includes("not signed at all")) return "";
    throw new Error(`codesign could not read the entitlements of ${target}: ${(res.stderr || res.stdout).trim()}`);
  }
  return res.stdout.trim();
}

/** Whitespace-insensitive equality of two entitlements dumps. */
export function sameEntitlements(a, b) {
  const squash = (xml) => xml.replace(/\s+/g, "");
  return squash(a) === squash(b);
}

function xmlEscape(value) {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

export function renderHelperInfoPlist({ version }) {
  const v = xmlEscape(version);
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>${HELPER_EXECUTABLE}</string>
  <key>CFBundleIdentifier</key>
  <string>${HELPER_BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>Ferryx Daemon</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${v}</string>
  <key>CFBundleVersion</key>
  <string>${v}</string>
  <key>LSUIElement</key>
  <true/>
</dict>
</plist>
`;
}

/**
 * Builds and signs <App>.app/Contents/Helpers/Ferryx Daemon.app on a finished app.
 *
 * - entitlements: where the outer app's recorded entitlements are written
 *   (conventionally <runDir>/outer-entitlements.plist). The file is always
 *   written; an empty file means the outer app has none.
 * - identity: codesign identity; "-" signs ad-hoc for debug/QA bundles.
 *
 * Returns { helperPath, version, entitlementsPath } where entitlementsPath is
 * null when the outer app carries no entitlements. Pass it to
 * finalizeMacosBundle({ entitlements }) so the outer app keeps them.
 */
export function assembleDaemonHelper({ appPath, identity, entitlements, exec = helperExec }) {
  if (!identity) throw new Error("assembleDaemonHelper requires a codesign identity (\"-\" for ad-hoc)");
  if (!entitlements) throw new Error("assembleDaemonHelper requires the path to record the outer entitlements");
  const appExecutable = join(appPath, "Contents", "MacOS", HELPER_EXECUTABLE);
  const appInfoPlist = join(appPath, "Contents", "Info.plist");
  if (!existsSync(appExecutable)) throw new Error(`app executable not found at ${appExecutable}`);
  if (!existsSync(appInfoPlist)) throw new Error(`Info.plist not found at ${appInfoPlist}`);

  // 1. Record the outer app's entitlements before anything inside it changes.
  const outerEntitlements = readEntitlementsXml(appPath, exec);
  mkdirSync(dirname(entitlements), { recursive: true });
  writeFileSync(entitlements, outerEntitlements === "" ? "" : `${outerEntitlements}\n`);
  const entitlementsPath = outerEntitlements === "" ? null : entitlements;

  const version = exec("plutil", ["-extract", "CFBundleShortVersionString", "raw", "-o", "-", appInfoPlist])
    .stdout.trim();
  if (!version) throw new Error(`CFBundleShortVersionString missing from ${appInfoPlist}`);

  // 2. Assemble the helper bundle from scratch (a rerun replaces a previous assembly).
  const helperPath = helperBundlePath(appPath);
  rmSync(helperPath, { recursive: true, force: true });
  const helperMacos = join(helperPath, "Contents", "MacOS");
  mkdirSync(helperMacos, { recursive: true });
  const helperExecutable = join(helperMacos, HELPER_EXECUTABLE);
  copyFileSync(appExecutable, helperExecutable);
  chmodSync(helperExecutable, 0o755);
  writeFileSync(join(helperPath, "Contents", "Info.plist"), renderHelperInfoPlist({ version }));

  // 3. Sign it as a bundle, carrying the outer app's entitlements when it has any.
  exec("codesign", [...codesignArgs(identity, entitlementsPath), helperPath]);

  return { helperPath, version, entitlementsPath };
}
