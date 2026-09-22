#!/usr/bin/env node
/**
 * Local notarized macOS build for Ferryx.
 *
 * Runs from a clean detached worktree at merged main HEAD:
 *   1. bun install for the UI workspace
 *   2. cargo tauri build --bundles app with APPLE_SIGNING_IDENTITY + updater key
 *      and an isolated CARGO_TARGET_DIR
 *   3. finalizeMacosBundle: re-sign every Mach-O (runtime + timestamp), seal,
 *      notarytool submit (profile FerryxNotary), assert Accepted, staple,
 *      validate, and require spctl "Notarized Developer ID"
 *
 * Prints the final notarized bundle path on success.
 */
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

const worktree = process.argv[2];
if (!worktree || !existsSync(path.join(worktree, "src-tauri/tauri.conf.json"))) {
  console.error("usage: node build-notarized-local.mjs <detached-worktree-dir>");
  process.exit(2);
}

const mainRepo = "/Volumes/T9-Mac/project/ferryx";
const signingIdentity = "Developer ID Application: Indo Yoon (5DUM8WPB4C)";
const notaryProfile = "FerryxNotary";
const targetDir = "/tmp/ferryx-notary-target";
const runDir = path.join(worktree, ".notary-run");

function run(cmd, args, extraEnv = {}, opts = {}) {
  const result = spawnSync(cmd, args, {
    encoding: "utf8",
    cwd: worktree,
    env: { ...process.env, ...extraEnv },
    stdio: opts.capture ? undefined : "inherit",
  });
  return {
    exit: result.status === null ? -1 : result.status,
    out: `${result.stdout ?? ""}${result.stderr ?? ""}`,
  };
}

// 1. UI dependencies for the fresh worktree.
const install = run("bun", ["install", "--cwd", path.join(worktree, "ui"), "--frozen-lockfile"]);
if (install.exit !== 0) {
  console.error(`bun install failed:\n${install.out.slice(-2000)}`);
  process.exit(1);
}

// 2. Signed build with the updater key from the repo-local .env (never committed).
const envText = readFileSync(path.join(mainRepo, ".env"), "utf8");
const updaterKey = /^TAURI_SIGNING_PRIVATE_KEY=(.*)$/m.exec(envText)?.[1]?.trim();
const updaterPassword = /^TAURI_SIGNING_PRIVATE_KEY_PASSWORD=(.*)$/m.exec(envText)?.[1]?.trim();
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

// 3. Finalize: re-sign, seal, notarize, staple, validate via the release pipeline.
const appPath = path.join(targetDir, "release/bundle/macos/Ferryx.app");
if (!existsSync(appPath)) {
  console.error(`bundle not found at ${appPath}`);
  process.exit(1);
}
mkdirWorkdir(runDir);
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
  exec: (cmd, args, opts = {}) => {
    const result = spawnSync(cmd, args, { encoding: "utf8" });
    return {
      status: result.status === null ? -1 : result.status,
      stdout: result.stdout ?? "",
      stderr: result.stderr ?? "",
    };
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
console.log(`notary submissions: ${JSON.stringify(outcome.notarizations)}`);

function mkdirWorkdir(dir) {
  spawnSync("mkdir", ["-p", dir]);
}
