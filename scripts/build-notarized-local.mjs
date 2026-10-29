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
import { existsSync, readFileSync } from "node:fs";
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

// 4. Finalize: re-sign, seal, notarize, staple, validate via the release pipeline.
const { finalizeMacosBundle } = await import(path.join(mainRepo, "scripts/lib/release-platforms.mjs"));
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

console.log(`NOTARIZED_BUNDLE:${appPath}`);
console.log(`DAEMON_HELPER:${helper.helperPath}`);
console.log(`notary submissions: ${JSON.stringify(outcome.notarizations)}`);

}

function mkdirWorkdir(dir) {
  spawnSync("mkdir", ["-p", dir]);
}
