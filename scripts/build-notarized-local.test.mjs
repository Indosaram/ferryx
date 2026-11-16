import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  DEFAULT_SIGNING_IDENTITY,
  main,
  parseArgs,
  preflight,
  updaterKeyFromEnv,
} from "./build-notarized-local.mjs";

const SECRET = "untrusted-test-secret-value";

function fakeHost({
  version = "2026.1022.6",
  installed = "2026.1022.5",
  dirty = "",
  helpers = 0,
  identity = true,
  envText = `TAURI_SIGNING_PRIVATE_KEY=${SECRET}\nTAURI_SIGNING_PRIVATE_KEY_PASSWORD=pw\n`,
  tauri = 0,
} = {}) {
  const calls = [];
  const files = {
    "/wt/src-tauri/tauri.conf.json": JSON.stringify({ version }),
    "/main/.env": envText,
  };
  return {
    calls,
    worktree: "/wt",
    mainRepo: "/main",
    env: {},
    readFile: (file) => {
      if (!(file in files)) throw new Error(`ENOENT ${file}`);
      return files[file];
    },
    exists: (file) => file in files || (file === "/Applications/Ferryx.app" && installed !== null),
    exec: (cmd, args) => {
      calls.push([cmd, ...args].join(" "));
      if (cmd === "cargo" && args[0] === "tauri") return { exit: tauri, out: "" };
      if (cmd === "git") return { exit: 0, out: dirty };
      if (cmd === "plutil") return { exit: 0, out: `${installed}\n` };
      if (cmd === "node") return { exit: helpers, out: helpers ? "fingerprint mismatch" : "verified" };
      if (cmd === "security") {
        return { exit: 0, out: identity ? `  1) ABC "${DEFAULT_SIGNING_IDENTITY}"\n` : "0 valid identities found\n" };
      }
      return { exit: 0, out: "" };
    },
  };
}

test("a ready host passes the preflight and checks the helper fingerprint", () => {
  const host = fakeHost();
  const result = preflight(host);
  assert.deepEqual(result.failures, []);
  assert.equal(result.ok, true);
  assert.ok(host.calls.some((call) => call.includes("build-remote-helpers.mjs --check-bundled /wt")));
});

test("an equal or older version than the installed app fails before the build", () => {
  const equal = preflight(fakeHost({ installed: "2026.1022.6" }));
  assert.equal(equal.ok, false);
  assert.match(equal.failures.join("\n"), /equals the installed version/);
  const older = preflight(fakeHost({ installed: "2026.1023.1" }));
  assert.match(older.failures.join("\n"), /older than the installed version/);
  const fresh = preflight(fakeHost({ installed: null }));
  assert.equal(fresh.ok, true, "no installed app is fine");
});

test("each missing prerequisite is named", () => {
  const result = preflight(
    fakeHost({ dirty: " M src/lib.rs", helpers: 1, identity: false, envText: "", tauri: 1 }),
  );
  const text = result.failures.join("\n");
  assert.match(text, /uncommitted changes/);
  assert.match(text, /bundled remote helpers do not match source/);
  assert.match(text, /TAURI_SIGNING_PRIVATE_KEY missing/);
  assert.match(text, /signing identity not in the keychain/);
  assert.match(text, /cargo tauri is not available/);
});

test("the updater key value never appears in preflight output", () => {
  const ok = preflight(fakeHost());
  const bad = preflight(fakeHost({ installed: "2026.1022.6", identity: false }));
  for (const result of [ok, bad]) {
    assert.doesNotMatch(JSON.stringify(result), new RegExp(SECRET));
  }
  assert.equal(updaterKeyFromEnv(`TAURI_SIGNING_PRIVATE_KEY=${SECRET}`).key, SECRET);
  assert.equal(updaterKeyFromEnv("").key, "");
});

// --- --finalize-only: sign a bundle compiled elsewhere, never compile it here ---------------

const APP = "/tmp/fixture/Ferryx.app";

/** A spawn stub that records every call and answers the codesign/spctl/security probes. */
function recordingSpawn({ cargoTauriBuild = 0 } = {}) {
  const calls = [];
  const spawn = (cmd, args) => {
    calls.push([cmd, ...args].join(" "));
    if (cmd === "cargo" && args[0] === "tauri" && args[1] === "build") return { status: cargoTauriBuild, stdout: "", stderr: "" };
    if (cmd === "security") return { status: 0, stdout: `  1) ABC "${DEFAULT_SIGNING_IDENTITY}"\n`, stderr: "" };
    if (cmd === "spctl") return { status: 0, stdout: "", stderr: "accepted\nsource=Notarized Developer ID\n" };
    return { status: 0, stdout: "", stderr: "" };
  };
  return { spawn, calls };
}

function finalizeFixture({ withPayload = true } = {}) {
  const root = mkdtempSync(join(tmpdir(), "ferryx-finalize-only-"));
  const appPath = join(root, "Ferryx.app");
  mkdirSync(join(appPath, "Contents", "MacOS"), { recursive: true });
  writeFileSync(join(appPath, "Contents", "Info.plist"), "<plist><dict/></plist>");
  writeFileSync(join(appPath, "Contents", "MacOS", "ferryx"), "binary");
  const ffmpegPayload = join(root, "payload", "aarch64-apple-darwin");
  if (withPayload) {
    mkdirSync(join(ffmpegPayload, "runtime"), { recursive: true });
    mkdirSync(join(ffmpegPayload, "legal"), { recursive: true });
    writeFileSync(join(ffmpegPayload, "runtime", "libavutil.60.dylib"), "dylib");
    writeFileSync(join(ffmpegPayload, "legal", "LICENSE.txt"), "lgpl");
    writeFileSync(
      join(ffmpegPayload, "stage.json"),
      JSON.stringify({ schemaVersion: 1, triple: "aarch64-apple-darwin", runtime: {}, legal: {} }),
    );
  }
  return { root, appPath, ffmpegPayload };
}

function finalizeDeps(overrides = {}) {
  const seen = { finalize: 0, assembly: 0, finalizeArgs: null };
  return {
    seen,
    env: { APPLE_SIGNING_IDENTITY: DEFAULT_SIGNING_IDENTITY, FERRYX_NOTARY_RUN_DIR: join(tmpdir(), "ferryx-notary-run-test") },
    log: () => {},
    error: () => {},
    verifyMacosFfmpegBundle: () => ["libavutil.60.dylib"],
    assembleDaemonHelper: ({ appPath }) => {
      seen.assembly += 1;
      return { helperPath: join(appPath, "Contents", "Helpers", "Ferryx Daemon.app", "manifest") };
    },
    finalizeMacosBundle: (args) => {
      seen.finalize += 1;
      seen.finalizeArgs = args;
      return { appPath: args.appPath, dmgPath: null, notarizations: [{ target: "app", id: "fixture", status: "Accepted" }] };
    },
    ...overrides,
  };
}

test("--finalize-only signs the copied bundle and never spawns a compiler", async () => {
  const { appPath, ffmpegPayload } = finalizeFixture();
  const { spawn, calls } = recordingSpawn();
  const deps = finalizeDeps();
  const code = await main(["--finalize-only", appPath, "--ffmpeg-payload", ffmpegPayload], { ...deps, spawn });

  assert.equal(code, 0);
  assert.equal(deps.seen.assembly, 1, "runs step 3, assembleDaemonHelper");
  assert.equal(deps.seen.finalize, 1, "runs step 4, finalizeMacosBundle");
  assert.ok(existsSync(join(appPath, "Contents", "Frameworks", "libavutil.60.dylib")), "runs step 3b, FFmpeg copy");
  assert.ok(
    calls.some((call) => call.startsWith(`codesign --force --options runtime --timestamp --sign ${DEFAULT_SIGNING_IDENTITY}`)),
    "runs step 3c, dylib signing",
  );
  assert.equal(deps.seen.finalizeArgs.approveNotarization, true, "submits to the notary service");
  assert.equal(deps.seen.finalizeArgs.notaryProfile, "FerryxNotary", "uses the notary keychain profile");
  assert.equal(deps.seen.finalizeArgs.signingIdentity, DEFAULT_SIGNING_IDENTITY, "signs with the Developer ID");
  assert.equal(
    deps.seen.finalizeArgs.entitlements,
    join(deps.env.FERRYX_NOTARY_RUN_DIR, "outer-entitlements.plist"),
    "hands the recorded entitlements to the final signing pass",
  );

  const compilers = calls.filter((call) => /^(cargo|rustc|tauri|bun|node|npx)\b/.test(call));
  assert.deepEqual(compilers, [], `finalize-only must not spawn a build command, saw: ${compilers.join("; ")}`);
  assert.ok(
    !calls.some((call) => /\btauri build\b/.test(call) || /\bbuild-remote-helpers\.mjs\b/.test(call)),
    "skips the preflight and the tauri build step",
  );
});

test("the build mode does spawn cargo tauri build, so the spy is not vacuous", async () => {
  const { root } = finalizeFixture();
  const worktree = join(root, "worktree");
  const repoDir = join(root, "repo");
  mkdirSync(join(worktree, "src-tauri"), { recursive: true });
  mkdirSync(join(worktree, "scripts"), { recursive: true });
  mkdirSync(repoDir, { recursive: true });
  writeFileSync(join(worktree, "src-tauri", "tauri.conf.json"), JSON.stringify({ version: "9999.999.9" }));
  writeFileSync(join(worktree, "scripts", "fetch-mhc.mjs"), "");
  writeFileSync(join(worktree, "scripts", "stage-ffmpeg.mjs"), "");
  writeFileSync(join(worktree, "scripts", "build-remote-helpers.mjs"), "");
  writeFileSync(join(repoDir, ".env"), `TAURI_SIGNING_PRIVATE_KEY=${SECRET}\n`);

  const { spawn, calls } = recordingSpawn({ cargoTauriBuild: 1 });
  const code = await main([worktree], { ...finalizeDeps(), spawn, repoDir });

  assert.equal(code, 1, "the failing build stops the run");
  assert.ok(calls.includes("cargo tauri build --bundles app"), `expected the build command, saw: ${calls.join("; ")}`);
});

test("--finalize-only on a bundle without the FFmpeg payload fails before notarizing", async () => {
  const { appPath, ffmpegPayload } = finalizeFixture({ withPayload: false });
  const { spawn, calls } = recordingSpawn();
  const errors = [];
  const deps = finalizeDeps({ error: (message) => errors.push(message) });
  const code = await main(["--finalize-only", appPath, "--ffmpeg-payload", ffmpegPayload], { ...deps, spawn });

  assert.equal(code, 1);
  assert.match(errors.join("\n"), /FFmpeg runtime payload missing/);
  assert.equal(deps.seen.finalize, 0, "no notarization submission");
  assert.ok(!calls.some((call) => call.startsWith("xcrun")), "no notarytool invocation");
});

test("--finalize-only refuses an incomplete command line and a non-bundle path", async () => {
  assert.equal(parseArgs(["--finalize-only", APP]).ok, false, "missing --ffmpeg-payload");
  assert.equal(parseArgs(["--finalize-only", APP, "--ffmpeg-payload"]).ok, false, "missing payload value");
  assert.equal(parseArgs(["/wt", "--finalize-only", APP, "--ffmpeg-payload", "/p"]).ok, false, "worktree with finalize-only");
  assert.equal(parseArgs(["/wt", "--bogus"]).ok, false, "unknown option");
  assert.equal(
    parseArgs(["--finalize-only", APP, "--ffmpeg-payload", "/p", "--check"]).ok,
    false,
    "--check belongs to the build preflight",
  );
  assert.deepEqual(parseArgs(["--finalize-only", APP, "--ffmpeg-payload", "/p"]), {
    ok: true, mode: "finalize-only", worktree: null, appPath: APP, ffmpegPayload: "/p", checkOnly: false,
  });

  const { spawn, calls } = recordingSpawn();
  const errors = [];
  const code = await main(["--finalize-only", "/nope/Ferryx.app", "--ffmpeg-payload", "/nope"], {
    ...finalizeDeps({ error: (message) => errors.push(message) }),
    spawn,
  });
  assert.equal(code, 1);
  assert.match(errors.join("\n"), /not an assembled Ferryx\.app bundle/);
  assert.deepEqual(calls, [], "nothing is spawned for a bad bundle path");
});
