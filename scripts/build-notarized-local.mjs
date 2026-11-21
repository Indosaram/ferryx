#!/usr/bin/env node
/**
 * Local notarized macOS build for Ferryx.
 *
 * Runs from a clean detached worktree at merged main HEAD:
 *   1. bun install for the UI workspace
 *   1b. bun scripts/fetch-mhc.mjs: the pinned, verified mhc + bun (Contents/Resources/mhc)
 *   2. cargo tauri build --bundles app with APPLE_SIGNING_IDENTITY + updater key
 *      and an isolated CARGO_TARGET_DIR
 *   3. assembleDaemonHelper: add the signed Contents/Helpers/Ferryx Daemon.app
 *      and record the outer app's entitlements
 *   4. finalizeMacosBundle: re-sign inside-out (loose Mach-O, nested helper
 *      bundles, outer app; runtime + timestamp, recorded entitlements), seal,
 *      notarytool submit (profile FerryxNotary), assert Accepted, staple,
 *      validate, and require spctl "Notarized Developer ID"
 *
 * A preflight runs first and fails in seconds instead of after the 20-minute build:
 * clean worktree, version newer than the installed app (an equal version never triggers
 * the daemon handover), bundled remote helpers match their source, the updater key is in
 * .env (its value is never printed), the signing identity is in the keychain, and the
 * build tools are on PATH. `--check` runs only the preflight.
 *
 *   node scripts/build-notarized-local.mjs <detached-worktree-dir> [--check]
 *
 * Prints the final notarized bundle path on success.
 */
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { realpathSync } from "node:fs";

import { evaluateVersionProgression, VERSION_REUSE_OVERRIDE_ENV } from "./install-macos-app.mjs";

export const DEFAULT_SIGNING_IDENTITY = "Developer ID Application: Indo Yoon (5DUM8WPB4C)";
export const INSTALLED_APP = "/Applications/Ferryx.app";

/** The updater key and password from the main repo .env. Values are returned, never logged. */
export function updaterKeyFromEnv(envText) {
  return {
    key: /^TAURI_SIGNING_PRIVATE_KEY=(.*)$/m.exec(envText ?? "")?.[1]?.trim() || "",
    password: /^TAURI_SIGNING_PRIVATE_KEY_PASSWORD=(.*)$/m.exec(envText ?? "")?.[1]?.trim() || "",
  };
}

/**
 * Checks everything the build needs before it starts. `exec(cmd, args, { cwd })` returns
 * `{ exit, out }`. Failure text names what is missing and never includes a secret value.
 */
export function preflight({
  worktree,
  mainRepo,
  exec,
  readFile = (file) => readFileSync(file, "utf8"),
  exists = existsSync,
  env = process.env,
  signingIdentity = DEFAULT_SIGNING_IDENTITY,
  installedApp = INSTALLED_APP,
}) {
  const failures = [];
  const notes = [];
  const confPath = path.join(worktree, "src-tauri/tauri.conf.json");
  let version = "";
  try {
    version = JSON.parse(readFile(confPath)).version ?? "";
  } catch (error) {
    failures.push(`cannot read ${confPath}: ${error.message}`);
  }

  for (const [tool, args] of [["bun", ["--version"]], ["cargo", ["--version"]], ["cargo", ["tauri", "--version"]]]) {
    if (exec(tool, args, { cwd: worktree }).exit !== 0) failures.push(`${tool} ${args[0]} is not available`);
  }

  const status = exec("git", ["status", "--porcelain", "--ignore-submodules=all", "--untracked-files=no"], { cwd: worktree });
  if (status.exit !== 0) failures.push("git status failed in the worktree");
  else if (status.out.trim()) failures.push(`worktree has uncommitted changes:\n${status.out.trimEnd()}`);

  if (version) {
    let installedVersion = "";
    if (exists(installedApp)) {
      const read = exec("plutil", ["-extract", "CFBundleShortVersionString", "raw", path.join(installedApp, "Contents/Info.plist")], {});
      installedVersion = read.exit === 0 ? read.out.trim() : "";
    }
    const progression = evaluateVersionProgression({
      incomingVersion: version,
      installedVersion,
      reuseOverride: env[VERSION_REUSE_OVERRIDE_ENV] === "1",
    });
    failures.push(...progression.failures);
    notes.push(`version ${version}${installedVersion ? ` (installed ${installedVersion})` : ""}`);
  }

  const helpers = exec("node", [path.join(worktree, "scripts/build-remote-helpers.mjs"), "--check-bundled", worktree], { cwd: worktree });
  if (helpers.exit !== 0) failures.push(`bundled remote helpers do not match source: ${helpers.out.trim().slice(-400)}`);

  const envFile = path.join(mainRepo, ".env");
  const envText = exists(envFile) ? readFile(envFile) : "";
  if (!updaterKeyFromEnv(envText).key) failures.push(`TAURI_SIGNING_PRIVATE_KEY missing from ${envFile}`);

  const identities = exec("security", ["find-identity", "-v", "-p", "codesigning"], {});
  if (identities.exit !== 0 || !identities.out.includes(signingIdentity)) {
    failures.push(`signing identity not in the keychain: ${signingIdentity}`);
  }
  return { ok: failures.length === 0, failures, notes, version };
}

function invokedDirectly() {
  if (!process.argv[1]) return false;
  try {
    return realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
}

if (invokedDirectly()) {
  await main();
}

async function main() {
const worktree = process.argv[2];
const checkOnly = process.argv.includes("--check");
if (!worktree || !existsSync(path.join(worktree, "src-tauri/tauri.conf.json"))) {
  console.error("usage: node build-notarized-local.mjs <detached-worktree-dir> [--check]");
  process.exit(2);
}

// This script lives at <main-repo>/scripts/build-notarized-local.mjs, so the repo holding the
// uncommitted .env and the release pipeline is the script's parent directory.
const mainRepo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const signingIdentity = process.env.APPLE_SIGNING_IDENTITY || DEFAULT_SIGNING_IDENTITY;
const notaryProfile = "FerryxNotary";
const targetDir = process.env.CARGO_TARGET_DIR || path.join(tmpdir(), "ferryx-notary-target");
const runDir = path.join(worktree, ".notary-run");

function run(cmd, args, extraEnv = {}, opts = {}) {
  const result = spawnSync(cmd, args, {
    encoding: "utf8",
    cwd: opts.cwd ?? worktree,
    env: { ...process.env, ...extraEnv },
    stdio: opts.capture ? undefined : "inherit",
  });
  return {
    exit: result.status === null ? -1 : result.status,
    out: `${result.stdout ?? ""}${result.stderr ?? ""}`,
  };
}

// 0. Preflight: fail before the long build.
const check = preflight({
  worktree,
  mainRepo,
  signingIdentity,
  exec: (cmd, args, opts = {}) => run(cmd, args, {}, { capture: true, cwd: opts.cwd }),
});
for (const note of check.notes) console.log(`preflight: ${note}`);
if (!check.ok) {
  for (const failure of check.failures) console.error(`preflight: ${failure}`);
  process.exit(1);
}
console.log("preflight: ok");
if (checkOnly) process.exit(0);

// 1. UI dependencies for the fresh worktree.
const install = run("bun", ["install", "--cwd", path.join(worktree, "ui"), "--frozen-lockfile"]);
if (install.exit !== 0) {
  console.error(`bun install failed:\n${install.out.slice(-2000)}`);
  process.exit(1);
}

// 1b. The pinned mhc + bun for this Mac's triple (verified; skipped when already present). The
// build's beforeBuildCommand runs the same fetch; doing it here fails fast before the long build.
const fetchMhc = run("bun", [path.join(worktree, "scripts/fetch-mhc.mjs"), "--prune"]);
if (fetchMhc.exit !== 0) {
  console.error("fetch-mhc failed: the bundled mhc could not be fetched and verified");
  process.exit(1);
}

// 1c. LGPL FFmpeg dylibs for the default `remote-desktop` feature. Without this the links against
// @rpath/libav*.dylib resolve to nothing, the bundle is not self-contained, and the daemon cannot
// start. Stage from the pinned SDK (scripts/fetch-ffmpeg.mjs) exactly as the release pipeline does.
const ffmpegTarget = "aarch64-apple-darwin";
const stageFfmpeg = run("bun", [
  path.join(worktree, "scripts/stage-ffmpeg.mjs"),
  "--target",
  ffmpegTarget,
  "--prefix",
  path.join(worktree, "src-tauri/resources/ffmpeg/sdk"),
]);
if (stageFfmpeg.exit !== 0) {
  console.error(`stage-ffmpeg failed: FFmpeg dylibs for ${ffmpegTarget} are required\n${stageFfmpeg.out.slice(-2000)}`);
  process.exit(1);
}

// 2. Signed build with the updater key from the repo-local .env (never committed).
const { key: updaterKey, password: updaterPassword } = updaterKeyFromEnv(
  readFileSync(path.join(mainRepo, ".env"), "utf8"),
);
if (!updaterKey) {
  console.error("TAURI_SIGNING_PRIVATE_KEY missing from main repo .env");
  process.exit(1);
}

const build = run(
  "cargo",
  ["tauri", "build", "--bundles", "app"],
  {
    APPLE_SIGNING_IDENTITY: signingIdentity,
    TAURI_SIGNING_PRIVATE_KEY: updaterKey,
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: updaterPassword ?? "",
    CARGO_TARGET_DIR: targetDir,
    // Phase 0: `profile.release` strips symbols and compiles without debug info, so dsymutil would
    // have nothing to archive and a crash report would name no function. Emit line tables, archive
    // the dSYM below, strip after the UUID gate passes, and only then sign and notarize.
    CARGO_PROFILE_RELEASE_STRIP: "none",
    CARGO_PROFILE_RELEASE_DEBUG: "1",
  },
);
if (build.exit !== 0) {
  console.error("cargo tauri build failed");
  process.exit(1);
}

// 3. Daemon helper bundle, before finalize so it is inside the notarized, stapled app.
const appPath = path.join(targetDir, "release/bundle/macos/Ferryx.app");
if (!existsSync(appPath)) {
  console.error(`bundle not found at ${appPath}`);
  process.exit(1);
}
mkdirWorkdir(runDir);

// 3. Keeper payload, before the dSYM gate and before anything seals the app. The daemon supervises
// keeper-owned PTY sessions, so a local notarized build that ships without the keeper is the same
// keeper-less supervisor the release path refuses; both paths assert it on the built bundle.
const { assertKeeperInApp, buildKeeperUniversal, installKeeperIntoApp, keeperExpectations } = await import(
  path.join(mainRepo, "scripts/lib/keeper-bundle.mjs")
);
// The rollback floor manifest the daemon reads at `Contents/Resources/keeper/latest.json`
// (`daemon::rollback::shipped_manifest_path`). Loaded from the same main-repo lib as the bundle
// helpers so both this path and the release pipeline share one derivation.
const { assertKeeperRollbackManifest, writeKeeperRollbackManifest } = await import(
  path.join(mainRepo, "scripts/lib/keeper-rollback-manifest.mjs")
);
const keeperDir = path.join(runDir, "keeper");
let keeperResourceDir;
try {
  // The generation name is the app version from the worktree's tauri.conf.json -- the same value
  // the daemon reads as `keeper_generation_version()`, so bundle and daemon agree on it.
  const appVersion = JSON.parse(
    readFileSync(path.join(worktree, "src-tauri", "tauri.conf.json"), "utf8"),
  ).version;
  // Built from the worktree being bundled, not the main repo: the keeper version and protocol
  // recorded in `keeper.json` must describe the daemon this app ships.
  buildKeeperUniversal({
    sourceDir: worktree,
    targetDir,
    destDir: keeperDir,
    triples: ["aarch64-apple-darwin", "x86_64-apple-darwin"],
    appVersion,
    // Same Phase 0 settings as the app build: without line tables dsymutil has nothing to extract
    // and the keeper would ship with no dSYM, which the gate below would then refuse.
    env: {
      ...process.env,
      CARGO_PROFILE_RELEASE_STRIP: "none",
      CARGO_PROFILE_RELEASE_DEBUG: "1",
    },
  });
  keeperResourceDir = installKeeperIntoApp({ appPath, stagedDir: keeperDir });
  // Both expectations, not just the generation name: the bundle is built from this worktree, so the
  // crate version its manifest records must be the one this source tree builds. Checking only
  // `expectedVersion` let a bundle built from other sources pass here.
  const keeper = assertKeeperInApp({
    appPath,
    ...keeperExpectations({ sourceDir: worktree, appVersion }),
  });
  // The rollback floor, from the ledger in the worktree being bundled. Written after the keeper
  // payload exists and asserted against that same ledger, before the dSYM gate and before
  // finalizeMacosBundle seals the bytes. Without it every rollback on this build would refuse with
  // KEEPER_ROLLBACK_FLOOR_UNVERIFIED, because the updater's latest.json is never bundled.
  writeKeeperRollbackManifest({ appPath, appVersion, sourceDir: worktree });
  assertKeeperRollbackManifest({ appPath, appVersion, sourceDir: worktree });
  console.log(`keeper: ${keeper.manifest.version} (protocol ${keeper.manifest.protocolMajor}.${keeper.manifest.protocolMinor})`);
} catch (error) {
  console.error(`keeper bundling failed: ${error.message}`);
  process.exit(1);
}

// 3a. Phase 0 dSYM gate on the built binary, before anything seals it. The dSYM must carry the
// SAME UUID as the bytes that ship, or a `ferryx +<offset>` frame can never be named; a dSYM from
// a different link fails this build instead of being archived and silently useless. The bundle is
// built from the per-architecture binaries (rustc packs a dSYM beside each), which is where the
// debug maps live for the lipo'd universal app binary.
const { assertDsymGate, isFile, produceDsymFromSlices, strippedForShipping } = await import(
  path.join(mainRepo, "scripts/lib/release-dsym.mjs")
);
// The archive describes itself in `dsyms/manifest.json`. Without it the release-symbolication gate
// can only refuse a run built this way with SYMBOLICATION_ARCHIVE_UNRECORDED, and provenance loses
// the keeper generation.
const { buildDsymArchiveManifest, writeDsymArchiveManifest } = await import(
  path.join(mainRepo, "scripts/lib/release-symbolication.mjs")
);
const dsymDir = path.join(runDir, "dsyms");
const shippedBinary = path.join(appPath, "Contents", "MacOS", "ferryx");
const targetTriples = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
const slices = targetTriples
  .map((triple) => path.join(targetDir, triple, "release/ferryx"))
  .filter((slice) => isFile(slice));
// The keeper ships inside the app, so its dSYM must be archived too: a SIGSEGV in the process that
// owns every PTY master is the crash this build most needs to name.
const keeperBinary = path.join(keeperResourceDir, "ferryx-ptykeeper");
const keeperSlices = targetTriples
  .map((triple) => path.join(targetDir, triple, "release/ferryx-ptykeeper"))
  .filter((slice) => isFile(slice));
let dsymGate;
let keeperDsymGate;
let ferryxDsym;
let keeperDsym;
let shippedKeeper;
let appVersion;
// The archive manifest is written AFTER finalizeMacosBundle: sealing re-signs the keeper, so a digest
// taken here would describe bytes that no longer exist once the bundle is sealed.
try {
  ferryxDsym = produceDsymFromSlices({
    slices: slices.length > 0 ? slices : [shippedBinary],
    outPath: path.join(dsymDir, "ferryx.dSYM"),
  });
  keeperDsym = produceDsymFromSlices({
    slices: keeperSlices.length > 0 ? keeperSlices : [keeperBinary],
    outPath: path.join(dsymDir, "ferryx-ptykeeper.dSYM"),
  });
  strippedForShipping(shippedBinary);
  dsymGate = assertDsymGate({ binaryPath: shippedBinary, dsymDir });
  strippedForShipping(keeperBinary);
  keeperDsymGate = assertDsymGate({ binaryPath: keeperBinary, dsymDir });
  appVersion = JSON.parse(readFileSync(path.join(worktree, "src-tauri", "tauri.conf.json"), "utf8")).version;
} catch (error) {
  console.error(`dSYM gate failed: ${error.message}`);
  process.exit(1);
}
console.log(`dSYM: ${dsymGate.binaryUuids.join(", ")} archived under ${dsymDir}`);
console.log(`dSYM: keeper ${keeperDsymGate.binaryUuids.join(", ")} archived under ${dsymDir}`);

const { assembleDaemonHelper } = await import(path.join(mainRepo, "scripts/macos-daemon-helper.mjs"));
let helper;
try {
  helper = assembleDaemonHelper({
    appPath,
    identity: signingIdentity,
    entitlements: path.join(runDir, "outer-entitlements.plist"),
  });
} catch (error) {
  console.error(`assembleDaemonHelper failed: ${error.message}`);
  process.exit(1);
}

// 3b. Copy the staged FFmpeg runtime into the bundle's Frameworks dir (the binary carries
// @executable_path/../Frameworks). Tauri does not do this for a plain `--bundles app` build.
const ffmpegPayload = path.join(worktree, "src-tauri/resources/ffmpeg/payload", ffmpegTarget);
const ffmpegRuntime = path.join(ffmpegPayload, "runtime");
const ffmpegLegal = path.join(ffmpegPayload, "legal");
const frameworksDir = path.join(appPath, "Contents/Frameworks");
const legalDir = path.join(appPath, "Contents/Resources/ffmpeg-legal");
if (!existsSync(ffmpegRuntime)) {
  console.error(`FFmpeg runtime payload missing at ${ffmpegRuntime}`);
  process.exit(1);
}
mkdirSync(frameworksDir, { recursive: true });
for (const name of readdirSync(ffmpegRuntime)) {
  if (!name.endsWith(".dylib")) continue;
  copyFileSync(path.join(ffmpegRuntime, name), path.join(frameworksDir, name));
}
if (existsSync(ffmpegLegal)) {
  mkdirSync(legalDir, { recursive: true });
  for (const name of readdirSync(ffmpegLegal)) {
    copyFileSync(path.join(ffmpegLegal, name), path.join(legalDir, name));
  }
}

// 3c. The dylibs are signed inside-out before the app, or codesign refuses the bundle.
for (const name of readdirSync(frameworksDir)) {
  if (!name.endsWith(".dylib")) continue;
  const sign = run("codesign", [
    "--force", "--options", "runtime", "--timestamp",
    "--sign", signingIdentity, path.join(frameworksDir, name),
  ], {}, { capture: true });
  if (sign.exit !== 0) {
    console.error(`codesign failed for ${name}:\n${sign.out.slice(-1500)}`);
    process.exit(1);
  }
}

// 4. Finalize: re-sign, seal, notarize, staple, validate via the release pipeline.
const { finalizeMacosBundle, verifyMacosFfmpegBundle } = await import(
  path.join(mainRepo, "scripts/lib/release-platforms.mjs")
);
const ffmpegDylibs = verifyMacosFfmpegBundle({ appPath, payloadDir: ffmpegPayload });
console.log(`ffmpeg: verified ${ffmpegDylibs.length} dylibs in Contents/Frameworks`);
let outcome;
try {
  outcome = finalizeMacosBundle({
  appPath,
  dmgPath: null,
  workspaceDir: runDir,
  signingIdentity,
  notaryProfile,
  approveNotarization: true,
  entitlements: path.join(runDir, "outer-entitlements.plist"),
  // The keeper was installed into this bundle in step 3 and ships in it; a seal without one would
  // produce the keeper-less supervisor both paths exist to forbid.
  requireKeeper: true,
  exec: (cmd, args, opts = {}) => {
    const result = spawnSync(cmd, args, { encoding: "utf8" });
    const outcome = {
      status: result.status === null ? -1 : result.status,
      stdout: result.stdout ?? "",
      stderr: result.stderr ?? "",
    };
    if (outcome.status !== 0 && !opts.allowFailure) {
      throw new Error(`${cmd} ${args.join(" ")} exited ${outcome.status}: ${(outcome.stderr || outcome.stdout).trim()}`);
    }
    return outcome;
  },
  });
} catch (error) {
  console.error(`finalizeMacosBundle failed: ${error.message}`);
  process.exit(1);
}

const spctl = spawnSync("spctl", ["-a", "-vvv", "-t", "exec", appPath], { encoding: "utf8" });
const combined = `${spctl.stdout}${spctl.stderr}`;
if (spctl.status !== 0 || !combined.includes("Notarized Developer ID")) {
  console.error(`spctl did not approve: ${combined.trim()}`);
  process.exit(1);
}

// 4a. The dSYM archive manifest, from the keeper digest finalizeMacosBundle re-derived on the SEALED
// bundle. Recording it before sealing (as this script used to) wrote a digest for bytes that code
// signing then rewrote, so the manifest, `keeper.json` and the shipped binary disagreed.
shippedKeeper = outcome.keeper;
try {
  writeDsymArchiveManifest(
    dsymDir,
    buildDsymArchiveManifest({
      runDir,
      appVersion,
      binaries: [
        { binary: shippedBinary, bundle: ferryxDsym.bundle, uuids: ferryxDsym.uuids, functions: ferryxDsym.functions },
        { binary: keeperBinary, bundle: keeperDsym.bundle, uuids: keeperDsym.uuids, functions: keeperDsym.functions },
      ],
      gates: [
        { ...dsymGate, binaryPath: shippedBinary },
        { ...keeperDsymGate, binaryPath: keeperBinary },
      ],
      keeper: shippedKeeper
        ? {
            version: shippedKeeper.version,
            protocolMajor: shippedKeeper.protocolMajor,
            protocolMinor: shippedKeeper.protocolMinor,
            binary: path.join(keeperResourceDir, "ferryx-ptykeeper"),
            // The keeper's own build UUIDs, so a crash report that names a keeper frame can be
            // paired with the archived dSYM without re-reading the archive. `ferryx daemon
            // crash-report` looks for the generation, then the UUID; recording both here is what
            // makes a keeper frame nameable months later.
            uuids: keeperDsym.uuids,
            sha256: shippedKeeper.sha256,
          }
        : null,
    }),
  );
  console.log(`dSYM: manifest ${path.join(dsymDir, "manifest.json")}`);
  // The archive ships beside the app, not inside it: `ferryx daemon crash-report` looks for a
  // `dsyms/` directory under the data dir, and the dSYM is diagnostics for the release rather than
  // an installable. Without this tarball a crash report from a local notarized build has nothing to
  // be symbolicated against, which is the state Phase 0 exists to end. `--no-xattrs` plus
  // `COPYFILE_DISABLE` keep AppleDouble entries out, matching what the release path writes.
  const dsymArchive = path.join(runDir, "Ferryx-universal.dSYM.tar.gz");
  const tarResult = spawnSync("tar", ["--no-xattrs", "-czf", dsymArchive, "-C", dsymDir, "."], {
    encoding: "utf8",
    env: { ...process.env, COPYFILE_DISABLE: "1" },
  });
  if (tarResult.status !== 0) {
    throw new Error(`dSYM archive failed: ${(tarResult.stderr ?? "").trim() || `exit ${tarResult.status}`}`);
  }
  console.log(`dSYM: archive ${dsymArchive}`);
} catch (error) {
  console.error(`dSYM archive manifest failed: ${error.message}`);
  process.exit(1);
}

console.log(`NOTARIZED_BUNDLE:${appPath}`);
console.log(`DAEMON_HELPER:${helper.helperPath}`);
console.log(`notary submissions: ${JSON.stringify(outcome.notarizations)}`);

}

function mkdirWorkdir(dir) {
  spawnSync("mkdir", ["-p", dir]);
}
