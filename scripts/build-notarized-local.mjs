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
 *   4. finalizeMacosBundle: re-sign inside-out (helper payload Mach-O, helper
 *      manifest refresh, remaining loose Mach-O, nested helper bundles, outer
 *      app last; runtime + timestamp, recorded entitlements), deep-verify the
 *      seal, notarytool submit (profile FerryxNotary), assert Accepted, staple,
 *      validate, and require spctl "Notarized Developer ID"
 *
 * A preflight runs first and fails in seconds instead of after the 20-minute build:
 * clean worktree, version newer than the installed app (an equal version never triggers
 * the daemon handover), bundled remote helpers match their source, the updater key is in
 * .env (its value is never printed), the signing identity is in the keychain, and the
 * build tools are on PATH. `--check` runs only the preflight.
 *
 *   bun scripts/build-notarized-local.mjs <detached-worktree-dir> [--check]
 *
 * The split release path compiles on the build host and signs here:
 *
 *   bun scripts/build-notarized-local.mjs --finalize-only <Ferryx.app> --ffmpeg-payload <payload-dir>
 *
 * `--finalize-only` skips steps 1-2 and the build preflight entirely: it never runs
 * `bun install`, a fetch script, or `cargo tauri build`. It runs steps 3, 3b (copy the
 * FFmpeg runtime into Contents/Frameworks), 3c (sign the dylibs) and 4 on the bundle that
 * was compiled on the build host and copied here. <payload-dir> is that host's
 * src-tauri/resources/ffmpeg/payload/aarch64-apple-darwin, i.e. the directory holding
 * runtime/, legal/ and stage.json. The notary keychain profile FerryxNotary must exist.
 *
 * Prints the final notarized bundle path on success.
 */
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { evaluateVersionProgression, VERSION_REUSE_OVERRIDE_ENV } from "./install-macos-app.mjs";

export const DEFAULT_SIGNING_IDENTITY = "Developer ID Application: Indo Yoon (5DUM8WPB4C)";
export const INSTALLED_APP = "/Applications/Ferryx.app";
export const NOTARY_PROFILE = "FerryxNotary";
export const FFMPEG_TARGET = "aarch64-apple-darwin";
export const USAGE =
  "bun scripts/build-notarized-local.mjs <detached-worktree-dir> [--check] | " +
  "bun scripts/build-notarized-local.mjs --finalize-only <Ferryx.app> --ffmpeg-payload <payload-dir>";

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

/**
 * Parses the command line (`process.argv.slice(2)`). Exactly one of the two modes is chosen:
 * a worktree to build from, or `--finalize-only <Ferryx.app> --ffmpeg-payload <dir>` to sign a
 * bundle that was compiled elsewhere.
 */
export function parseArgs(args) {
  const parsed = { mode: "build", worktree: null, appPath: null, ffmpegPayload: null, checkOnly: false };
  const rest = [...args];
  const fail = (error) => ({ ok: false, error, usage: USAGE });
  while (rest.length) {
    const arg = rest.shift();
    if (arg === "--check") parsed.checkOnly = true;
    else if (arg === "--finalize-only") parsed.appPath = rest.shift() ?? "";
    else if (arg === "--ffmpeg-payload") parsed.ffmpegPayload = rest.shift() ?? "";
    else if (arg.startsWith("--")) return fail(`unknown option ${arg}`);
    else if (parsed.worktree === null) parsed.worktree = arg;
    else return fail(`unexpected argument ${arg}`);
  }
  if (parsed.appPath === null) {
    if (!parsed.worktree) return fail("missing <detached-worktree-dir>");
    return { ok: true, ...parsed };
  }
  parsed.mode = "finalize-only";
  if (!parsed.appPath) return fail("--finalize-only needs the path to a .app bundle");
  if (!parsed.ffmpegPayload) return fail("--finalize-only needs --ffmpeg-payload <payload-dir>");
  if (parsed.worktree) return fail(`--finalize-only takes no worktree (got ${parsed.worktree})`);
  if (parsed.checkOnly) return fail("--check is a build-mode preflight; --finalize-only has no preflight");
  return { ok: true, ...parsed };
}

/**
 * One spawn seam for the whole pipeline: `spawn` is `child_process.spawnSync` in production and
 * a recording stub in tests, so a test can prove which commands a mode runs.
 */
export function makeRun({ spawn = spawnSync, cwd = process.cwd(), env = process.env } = {}) {
  return (cmd, args, extraEnv = {}, opts = {}) => {
    const result = spawn(cmd, args, {
      encoding: "utf8",
      cwd: opts.cwd ?? cwd,
      env: { ...env, ...extraEnv },
      stdio: opts.capture ? undefined : "inherit",
    });
    const stdout = result.stdout ?? "";
    const stderr = result.stderr ?? "";
    return { exit: result.status === null ? -1 : result.status, out: `${stdout}${stderr}`, stdout, stderr };
  };
}

/** `exec(command, args, { allowFailure })` over a `run` seam, the shape scripts/lib/release-platforms.mjs expects. */
export function execFromRun(run) {
  return (command, args, { allowFailure = false } = {}) => {
    const { exit, out, stdout, stderr } = run(command, args, {}, { capture: true });
    if (exit !== 0 && !allowFailure) {
      throw new Error(`Command failed (exit ${exit}): ${command} ${args.join(" ")}\n${out.trim()}`);
    }
    return { status: exit, stdout, stderr };
  };
}

/**
 * Steps 3, 3b, 3c and 4 of the release: helper bundle, FFmpeg runtime into Contents/Frameworks,
 * inside-out dylib signing, then finalizeMacosBundle (re-sign, notarize, staple, validate) and
 * the final spctl assessment. Shared by the full build and by `--finalize-only`, so the split
 * path finalizes exactly the bundle the build path would have finalized. Compiles nothing.
 */
export async function finalizeLocally({
  appPath,
  ffmpegPayload,
  signingIdentity,
  notaryProfile = NOTARY_PROFILE,
  runDir,
  run,
  log = console.log,
  repoDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), ".."),
  assembleDaemonHelper,
  finalizeMacosBundle,
  verifyMacosFfmpegBundle,
}) {
  if (!existsSync(path.join(appPath, "Contents/MacOS/ferryx"))) {
    throw new Error(`bundle not found at ${appPath}`);
  }
  const exec = execFromRun(run);
  const load = async (relative, exported) => (await import(pathToFileURL(path.join(repoDir, relative)).href))[exported];
  mkdirSync(runDir, { recursive: true });

  // 3. Daemon helper bundle, before finalize so it is inside the notarized, stapled app.
  const assemble = assembleDaemonHelper ?? (await load("scripts/macos-daemon-helper.mjs", "assembleDaemonHelper"));
  const entitlements = path.join(runDir, "outer-entitlements.plist");
  let helper;
  try {
    helper = assemble({ appPath, identity: signingIdentity, entitlements, exec });
  } catch (error) {
    throw new Error(`assembleDaemonHelper failed: ${error.message}`);
  }

  // 3b. Copy the staged FFmpeg runtime into the bundle's Frameworks dir (the binary carries
  // @executable_path/../Frameworks). Tauri does not do this for a plain `--bundles app` build.
  const ffmpegRuntime = path.join(ffmpegPayload, "runtime");
  const ffmpegLegal = path.join(ffmpegPayload, "legal");
  const frameworksDir = path.join(appPath, "Contents/Frameworks");
  const legalDir = path.join(appPath, "Contents/Resources/ffmpeg-legal");
  if (!existsSync(ffmpegRuntime)) {
    throw new Error(`FFmpeg runtime payload missing at ${ffmpegRuntime}`);
  }
  if (!existsSync(path.join(ffmpegPayload, "stage.json"))) {
    throw new Error(
      `FFmpeg payload manifest missing at ${path.join(ffmpegPayload, "stage.json")} ` +
        `(pass the build host's src-tauri/resources/ffmpeg/payload/${FFMPEG_TARGET})`,
    );
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
      throw new Error(`codesign failed for ${name}:\n${sign.out.slice(-1500)}`);
    }
  }

  // 4. Finalize: re-sign, seal, notarize, staple, validate via the release pipeline.
  const verify =
    verifyMacosFfmpegBundle ?? (await load("scripts/lib/release-platforms.mjs", "verifyMacosFfmpegBundle"));
  const finalize = finalizeMacosBundle ?? (await load("scripts/lib/release-platforms.mjs", "finalizeMacosBundle"));
  const ffmpegDylibs = verify({ appPath, payloadDir: ffmpegPayload, exec });
  log(`ffmpeg: verified ${ffmpegDylibs.length} dylibs in Contents/Frameworks`);
  let outcome;
  try {
    outcome = finalize({
      appPath,
      dmgPath: null,
      workspaceDir: runDir,
      signingIdentity,
      notaryProfile,
      approveNotarization: true,
      entitlements,
      exec,
    });
  } catch (error) {
    throw new Error(`finalizeMacosBundle failed: ${error.message}`);
  }

  // 4b. The script's own Gatekeeper gate: the finalized bundle must assess as Notarized Developer ID.
  const spctl = run("spctl", ["-a", "-vvv", "-t", "exec", appPath], {}, { capture: true });
  if (spctl.exit !== 0 || !spctl.out.includes("Notarized Developer ID")) {
    throw new Error(`spctl did not approve: ${spctl.out.trim()}`);
  }

  return { appPath, helperPath: helper.helperPath, notarizations: outcome.notarizations };
}

/**
 * Entry point. Returns the process exit code; `deps` exists so tests can record every spawn
 * (`spawn`) and keep environment and module loading deterministic.
 */
export async function main(argv = process.argv.slice(2), deps = {}) {
  const log = deps.log ?? console.log;
  const printError = deps.error ?? console.error;
  const env = deps.env ?? process.env;
  const spawn = deps.spawn ?? spawnSync;
  const cli = parseArgs(argv);
  if (!cli.ok) {
    printError(cli.error);
    printError(USAGE);
    return 2;
  }
  const repoDir = deps.repoDir ?? path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const signingIdentity = env.APPLE_SIGNING_IDENTITY || DEFAULT_SIGNING_IDENTITY;
  try {
    if (cli.mode === "finalize-only") {
      return await finalizeOnlyMain({ cli, signingIdentity, repoDir, spawn, env, log, deps });
    }
    if (!existsSync(path.join(cli.worktree, "src-tauri/tauri.conf.json"))) {
      printError(`not a Ferryx worktree: ${cli.worktree} (missing src-tauri/tauri.conf.json)`);
      printError(USAGE);
      return 2;
    }
    return await buildMain({ cli, signingIdentity, repoDir, spawn, env, log, printError, deps });
  } catch (error) {
    printError(error.message);
    return 1;
  }
}

/** `--finalize-only`: sign, notarize, staple and verify a bundle compiled on the build host. */
async function finalizeOnlyMain({ cli, signingIdentity, repoDir, spawn, env, log, deps }) {
  const appPath = path.resolve(cli.appPath);
  if (!existsSync(path.join(appPath, "Contents/Info.plist")) || !existsSync(path.join(appPath, "Contents/MacOS/ferryx"))) {
    throw new Error(
      `not an assembled Ferryx.app bundle: ${appPath} (missing Contents/Info.plist or Contents/MacOS/ferryx)`,
    );
  }
  const ffmpegPayload = path.resolve(cli.ffmpegPayload);
  if (!existsSync(path.join(ffmpegPayload, "runtime"))) {
    throw new Error(`FFmpeg runtime payload missing at ${path.join(ffmpegPayload, "runtime")}`);
  }

  // This Mac's codesign needs the private key behind the identity; check before the first copy.
  const run = makeRun({ cwd: path.dirname(appPath), spawn, env });
  if (signingIdentity !== "-") {
    const identities = run("security", ["find-identity", "-v", "-p", "codesigning"], {}, { capture: true });
    if (identities.exit !== 0 || !identities.out.includes(signingIdentity)) {
      throw new Error(`signing identity not in the keychain: ${signingIdentity}`);
    }
  }

  const runDir = env.FERRYX_NOTARY_RUN_DIR || path.join(tmpdir(), "ferryx-notary-run");
  const outcome = await finalizeLocally({
    appPath,
    ffmpegPayload,
    signingIdentity,
    notaryProfile: NOTARY_PROFILE,
    runDir,
    run,
    log,
    repoDir,
    assembleDaemonHelper: deps.assembleDaemonHelper,
    finalizeMacosBundle: deps.finalizeMacosBundle,
    verifyMacosFfmpegBundle: deps.verifyMacosFfmpegBundle,
  });
  log(`NOTARIZED_BUNDLE:${outcome.appPath}`);
  log(`DAEMON_HELPER:${outcome.helperPath}`);
  log(`notary submissions: ${JSON.stringify(outcome.notarizations)}`);
  return 0;
}

/** The full local build: preflight, UI deps, mhc, FFmpeg, `cargo tauri build`, then finalize. */
async function buildMain({ cli, signingIdentity, repoDir, spawn, env, log, printError, deps }) {
  const worktree = cli.worktree;
  const run = makeRun({ cwd: worktree, spawn, env });
  const targetDir = env.CARGO_TARGET_DIR || path.join(tmpdir(), "ferryx-notary-target");
  const runDir = path.join(worktree, ".notary-run");

  // 0. Preflight: fail before the long build.
  const check = preflight({
    worktree,
    mainRepo: repoDir,
    signingIdentity,
    env,
    exec: (cmd, args, opts = {}) => run(cmd, args, {}, { capture: true, cwd: opts.cwd }),
  });
  for (const note of check.notes) log(`preflight: ${note}`);
  if (!check.ok) {
    for (const failure of check.failures) printError(`preflight: ${failure}`);
    return 1;
  }
  log("preflight: ok");
  if (cli.checkOnly) return 0;

  // 1. UI dependencies for the fresh worktree.
  const install = run("bun", ["install", "--cwd", path.join(worktree, "ui"), "--frozen-lockfile"]);
  if (install.exit !== 0) {
    throw new Error(`bun install failed:\n${install.out.slice(-2000)}`);
  }

  // 1b. The pinned mhc + bun for this Mac's triple (verified; skipped when already present). The
  // build's beforeBuildCommand runs the same fetch; doing it here fails fast before the long build.
  const fetchMhc = run("bun", [path.join(worktree, "scripts/fetch-mhc.mjs"), "--prune"]);
  if (fetchMhc.exit !== 0) {
    throw new Error("fetch-mhc failed: the bundled mhc could not be fetched and verified");
  }

  // 1c. LGPL FFmpeg dylibs for the default `remote-desktop` feature. Without this the links against
  // @rpath/libav*.dylib resolve to nothing, the bundle is not self-contained, and the daemon cannot
  // start. Stage from the pinned SDK (scripts/fetch-ffmpeg.mjs) exactly as the release pipeline does.
  const stageFfmpeg = run("bun", [
    path.join(worktree, "scripts/stage-ffmpeg.mjs"),
    "--target",
    FFMPEG_TARGET,
    "--prefix",
    path.join(worktree, "src-tauri/resources/ffmpeg/sdk"),
  ]);
  if (stageFfmpeg.exit !== 0) {
    throw new Error(`stage-ffmpeg failed: FFmpeg dylibs for ${FFMPEG_TARGET} are required\n${stageFfmpeg.out.slice(-2000)}`);
  }

  // 2. Signed build with the updater key from the repo-local .env (never committed).
  const { key: updaterKey, password: updaterPassword } = updaterKeyFromEnv(
    readFileSync(path.join(repoDir, ".env"), "utf8"),
  );
  if (!updaterKey) {
    throw new Error("TAURI_SIGNING_PRIVATE_KEY missing from main repo .env");
  }

  const build = run(
    "cargo",
    ["tauri", "build", "--bundles", "app"],
    {
      APPLE_SIGNING_IDENTITY: signingIdentity,
      TAURI_SIGNING_PRIVATE_KEY: updaterKey,
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: updaterPassword ?? "",
      CARGO_TARGET_DIR: targetDir,
    },
  );
  if (build.exit !== 0) {
    throw new Error("cargo tauri build failed");
  }

  const appPath = path.join(targetDir, "release/bundle/macos/Ferryx.app");
  if (!existsSync(appPath)) {
    throw new Error(`bundle not found at ${appPath}`);
  }
  const outcome = await finalizeLocally({
    appPath,
    ffmpegPayload: path.join(worktree, "src-tauri/resources/ffmpeg/payload", FFMPEG_TARGET),
    signingIdentity,
    notaryProfile: NOTARY_PROFILE,
    runDir,
    run,
    log,
    repoDir,
    assembleDaemonHelper: deps.assembleDaemonHelper,
    finalizeMacosBundle: deps.finalizeMacosBundle,
    verifyMacosFfmpegBundle: deps.verifyMacosFfmpegBundle,
  });
  log(`NOTARIZED_BUNDLE:${outcome.appPath}`);
  log(`DAEMON_HELPER:${outcome.helperPath}`);
  log(`notary submissions: ${JSON.stringify(outcome.notarizations)}`);
  return 0;
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
  const code = await main();
  if (code !== 0) process.exit(code);
}
