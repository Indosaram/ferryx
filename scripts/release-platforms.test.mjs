import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync, existsSync, readFileSync, chmodSync, readdirSync, statSync, cpSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { createHash, generateKeyPairSync, randomBytes, sign } from "node:crypto";

import {
  probeMacbook,
  probeOmaki,
  probeMahoWin,
  preflightAll,
  createGitBundles,
  createBuildReceipt,
  createLinuxBuildScript,
  createWindowsBuildScript,
  buildHost,
  signUpdaterArtifact,
  submitForNotarization,
  recreateDmgFromApp,
  finalizeMacosBundle,
} from "./lib/release-platforms.mjs";
import { parseReceipt } from "./lib/release-contract.mjs";

test("release-platforms: exports required APIs", () => {
  assert.equal(typeof probeMacbook, "function");
  assert.equal(typeof probeOmaki, "function");
  assert.equal(typeof probeMahoWin, "function");
  assert.equal(typeof preflightAll, "function");
  assert.equal(typeof createGitBundles, "function");
  assert.equal(typeof createBuildReceipt, "function");
  assert.equal(typeof createLinuxBuildScript, "function");
  assert.equal(typeof createWindowsBuildScript, "function");
  assert.equal(typeof buildHost, "function");
  assert.equal(typeof submitForNotarization, "function");
  assert.equal(typeof recreateDmgFromApp, "function");
  assert.equal(typeof finalizeMacosBundle, "function");
});

test("buildHost: remote artifact transport uses keepalive options", async () => {
  const source = readFileSync(new URL("./lib/release-platforms.mjs", import.meta.url), "utf8");
  assert.match(source, /ConnectTimeout=30/);
  assert.match(source, /ServerAliveInterval=15/);
  assert.match(source, /ServerAliveCountMax=6/);
});

test("probeMacbook: correctly inspects disk budget and detects insufficiency", async () => {
  const hostConfig = {
    platform: "darwin",
    ssh: null,
    root: "/Users/indo/ferryx-release-builds",
    minFreeBytes: 100 * 1024 * 1024 * 1024 * 1024, // 100 TB - impossible to satisfy
    signingIdentity: "Developer ID Application: Indo Yoon (5DUM8WPB4C)",
    notaryProfile: "FerryxNotary",
  };

  const result = await probeMacbook(hostConfig);
  assert.equal(result.host, "macbook");
  assert.equal(result.platform, "darwin");
  assert.equal(result.reachable, true);
  assert.equal(result.disk.ok, false);
  assert.ok(result.disk.availableBytes > 0);
  assert.ok(result.failures.some((f) => f.includes("disk")));
});

test("probeMacbook: passes disk check when budget is conservative", async () => {
  const hostConfig = {
    platform: "darwin",
    ssh: null,
    root: "/Users/indo/ferryx-release-builds",
    minFreeBytes: 1024 * 1024, // 1 MB
    signingIdentity: "Developer ID Application: Indo Yoon (5DUM8WPB4C)",
    notaryProfile: "FerryxNotary",
  };

  const result = await probeMacbook(hostConfig);
  assert.equal(result.host, "macbook");
  assert.equal(result.reachable, true);
  assert.equal(result.disk.ok, true);
  assert.ok(typeof result.signing.hasSigningIdentity === "boolean");
  assert.ok(typeof result.signing.hasNotaryProfile === "boolean");
  // Ensure no secret or credential is leaked in the signing result
  assert.equal(Object.keys(result.signing).length, 2);
});

test("createGitBundles: produces valid standalone git bundles and sha256 digests", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-bundle-test-"));
  try {
    const repoA = join(tmp, "repo-a");
    mkdirSync(repoA);
    execFileSync("git", ["init"], { cwd: repoA });
    execFileSync("git", ["config", "user.name", "Test"], { cwd: repoA });
    execFileSync("git", ["config", "user.email", "test@test.com"], { cwd: repoA });
    writeFileSync(join(repoA, "README.md"), "Hello");
    execFileSync("git", ["add", "README.md"], { cwd: repoA });
    execFileSync("git", ["commit", "-m", "Initial commit"], { cwd: repoA });
    const shaA = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoA, encoding: "utf8" }).trim();

    const repoB = join(tmp, "repo-b");
    mkdirSync(repoB);
    execFileSync("git", ["init"], { cwd: repoB });
    execFileSync("git", ["config", "user.name", "Test"], { cwd: repoB });
    execFileSync("git", ["config", "user.email", "test@test.com"], { cwd: repoB });
    writeFileSync(join(repoB, "ghostty.txt"), "GhosttyVT");
    execFileSync("git", ["add", "ghostty.txt"], { cwd: repoB });
    execFileSync("git", ["commit", "-m", "Ghostty pin"], { cwd: repoB });
    const shaB = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoB, encoding: "utf8" }).trim();

    const bundlesDir = join(tmp, "bundles");
    mkdirSync(bundlesDir);

    const bundleResult = createGitBundles({
      repoDir: repoA,
      ghosttyRepoDir: repoB,
      commitSha: shaA,
      ghosttyPin: shaB,
      outDir: bundlesDir,
    });

    assert.ok(existsSync(bundleResult.sourceBundlePath));
    assert.ok(existsSync(bundleResult.ghosttyBundlePath));
    assert.match(bundleResult.sourceBundleSha256, /^[0-9a-f]{64}$/);
    assert.match(bundleResult.ghosttyBundleSha256, /^[0-9a-f]{64}$/);

    // Verify bundle can be cloned in isolated directory
    const testClone = join(tmp, "clone-test");
    execFileSync("git", ["clone", bundleResult.sourceBundlePath, testClone]);
    const clonedSha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: testClone, encoding: "utf8" }).trim();
    assert.equal(clonedSha, shaA);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("createBuildReceipt: creates valid receipt conforming to release contract", () => {
  const plan = {
    schemaVersion: 1,
    runId: "rel-2026.09.08-1",
    repo: "Indosaram/ferryx",
    commitSha: "5d5499806a1b207849778f488b4e3e7b821a751b",
    tag: "v2026.09.08.1",
    appVersion: "2026.908.1",
    msixVersion: "2026.908.1.0",
    channels: { store: true, nsisMigration: false },
    requiredTargets: ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"],
    toolchains: { node: ">=22.0.0", bun: ">=1.4.0", zig: "0.16.0" },
    sourceDateEpoch: 1788868800,
    createdAt: "2026-09-08T12:00:00.000Z",
  };

  const receipt = createBuildReceipt({
    host: "macbook",
    runId: plan.runId,
    commitSha: plan.commitSha,
    appVersion: plan.appVersion,
    exitCode: 0,
    artifacts: [
      {
        kind: "macos-updater",
        name: "Ferryx.app.tar.gz",
        relPath: "darwin/Ferryx.app.tar.gz",
        bytes: 12345,
        sha256: "a".repeat(64),
        signatureRelPath: "darwin/Ferryx.app.tar.gz.sig",
        targets: ["darwin-aarch64", "darwin-x86_64"],
      },
      {
        kind: "dmg",
        name: "Ferryx_2026.908.1_universal.dmg",
        relPath: "darwin/Ferryx_2026.908.1_universal.dmg",
        bytes: 23456,
        sha256: "b".repeat(64),
        signatureRelPath: null,
        targets: [],
      },
    ],
    toolchains: {
      node: "22.22.3",
      bun: "1.4.0",
      zig: "0.16.0",
      rust: "1.92.0",
      tauri: "2.10.1",
    },
  });

  const parsed = parseReceipt(receipt, plan);
  assert.equal(parsed.host, "macbook");
  assert.equal(parsed.runId, plan.runId);
  assert.equal(parsed.artifacts.length, 2);
  assert.equal(parsed.toolchains.bun, "1.4.0");
});

test("remote builders: generated scripts enforce exact isolated build and artifact contracts", () => {
  const plan = {
    commitSha: "5d5499806a1b207849778f488b4e3e7b821a751b",
    tag: "v2026.09.08.1", msixVersion: "2026.908.1.0", sourceDateEpoch: 1788868800,
    channels: { nsisMigration: true },
  };
  const linux = createLinuxBuildScript({
    workspaceDir: "/tmp/release with quote'", plan,
    hostConfig: { path: "/opt/zig bin" },
  });
  assert.match(linux, /git bundle verify/);
  assert.match(linux, /checkout --detach/);
  assert.match(linux, /--frozen-lockfile/);
  assert.match(linux, /appimage,deb/);
  assert.match(linux, /cp .*Ferryx_amd64\.AppImage/);
  assert.doesNotMatch(linux, /AppImage\.tar\.gz/);
  assert.doesNotMatch(linux, /signer sign/);
  assert.doesNotMatch(linux, /signer --help/);
  assert.match(linux, /test -s/);

  const windows = createWindowsBuildScript({
    workspaceDir: "C:/release root/run", plan,
    hostConfig: { path: "C:/zig bin" },
  });
  assert.match(windows, /git bundle verify/);
  assert.match(windows, /--frozen-lockfile/);
  assert.match(windows, /build-msix\.ps1/);
  assert.match(windows, /-ExePath/);
  assert.match(windows, /-SkipSigning/);
  assert.match(windows, /--bundles nsis/);
  assert.doesNotMatch(windows, /signer sign/);
  assert.doesNotMatch(windows, /signer --help/);
});

test("buildHost: fails closed when a custom runner returns no result", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-buildhost-runner-result-"));
  try {
    const runDir = join(tmp, "run");
    const hostRoot = join(tmp, "host-root");
    mkdirSync(join(runDir, "bundles"), { recursive: true });
    mkdirSync(hostRoot);
    writeFileSync(join(runDir, "bundles", "source.bundle"), "fixture");
    writeFileSync(join(runDir, "bundles", "ghostty.bundle"), "fixture");
    writeFileSync(join(runDir, "plan.json"), JSON.stringify({
      schemaVersion: 1, runId: "rel-run-result-1", repo: "Indosaram/ferryx",
      commitSha: "5d5499806a1b207849778f488b4e3e7b821a751b", tag: "v2026.09.08.1",
      appVersion: "2026.908.1", msixVersion: "2026.908.1.0",
      channels: { store: true, nsisMigration: false },
      requiredTargets: ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"],
      toolchains: { node: ">=22.0.0", bun: ">=1.4.0", zig: "0.16.0" },
      sourceDateEpoch: 1788868800, createdAt: "2026-09-08T12:00:00.000Z",
    }));
    const config = { repository: tmp, ghosttyRepository: tmp, hosts: {
      macbook: { platform: "darwin", ssh: null, root: hostRoot, minFreeBytes: 1 },
    }};

    await assert.rejects(
      () => buildHost({ hostName: "macbook", config, runDir, runner: async () => undefined }),
      /runner returned no result/i,
    );
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("buildHost: rejects unknown host or missing run directory", async () => {
  const config = {
    schemaVersion: 1,
    repository: "/Users/indo/code/project/orca-lite",
    ghosttyRepository: "/Users/indo/code/project/orca-lite/src-tauri/vendor/ghostty",
    repo: "Indosaram/ferryx",
    hosts: {
      macbook: {
        platform: "darwin",
        ssh: null,
        root: "/Users/indo/ferryx-release-builds",
        minFreeBytes: 1024,
      },
    },
  };

  await assert.rejects(
    () => buildHost({ hostName: "invalid-host", config, runDir: "/tmp/nonexistent" }),
    /Unknown host: 'invalid-host'/,
  );
});

test("buildHost: fails closed if workspace already exists on host", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-buildhost-clobber-"));
  try {
    const runDir = join(tmp, "run");
    mkdirSync(runDir);
    const hostRoot = join(tmp, "host-root");
    mkdirSync(hostRoot);

    const plan = {
      schemaVersion: 1,
      runId: "rel-run-clobber-1",
      repo: "Indosaram/ferryx",
      commitSha: "5d5499806a1b207849778f488b4e3e7b821a751b",
      tag: "v2026.09.08.1",
      appVersion: "2026.908.1",
      msixVersion: "2026.908.1.0",
      channels: { store: true, nsisMigration: false },
      requiredTargets: ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"],
      toolchains: { node: ">=22.0.0", bun: ">=1.4.0", zig: "0.16.0" },
      sourceDateEpoch: 1788868800,
      createdAt: "2026-09-08T12:00:00.000Z",
    };
    writeFileSync(join(runDir, "plan.json"), JSON.stringify(plan));

    // Pre-create the workspace to trigger collision / clobber error
    mkdirSync(join(hostRoot, plan.runId));

    const config = {
      schemaVersion: 1,
      repository: "/Users/indo/code/project/orca-lite",
      ghosttyRepository: "/Users/indo/code/project/orca-lite/src-tauri/vendor/ghostty",
      repo: "Indosaram/ferryx",
      hosts: {
        macbook: {
          platform: "darwin",
          ssh: null,
          root: hostRoot,
          minFreeBytes: 1024,
        },
      },
    };

    await assert.rejects(
      () => buildHost({ hostName: "macbook", config, runDir }),
      /Host workspace already exists: refusing to overwrite/,
    );
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("buildHost: executes builder fixture, stages artifacts, and writes valid receipt", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-buildhost-fixture-"));
  try {
    const runDir = join(tmp, "run");
    mkdirSync(runDir);
    mkdirSync(join(runDir, "artifacts"));
    mkdirSync(join(runDir, "artifacts", "darwin"));
    mkdirSync(join(runDir, "receipts"));
    mkdirSync(join(runDir, "bundles"));
    const hostRoot = join(tmp, "host-root");
    mkdirSync(hostRoot);

    // Provide pre-created fixture bundles so unit test doesn't re-bundle whole gigabyte repos
    writeFileSync(join(runDir, "bundles", "source.bundle"), "FIXTURE_SOURCE_BUNDLE");
    writeFileSync(join(runDir, "bundles", "ghostty.bundle"), "FIXTURE_GHOSTTY_BUNDLE");

    const plan = {
      schemaVersion: 1,
      runId: "rel-run-fixture-1",
      repo: "Indosaram/ferryx",
      commitSha: "5d5499806a1b207849778f488b4e3e7b821a751b",
      tag: "v2026.09.08.1",
      appVersion: "2026.908.1",
      msixVersion: "2026.908.1.0",
      channels: { store: true, nsisMigration: false },
      requiredTargets: ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"],
      toolchains: { node: ">=22.0.0", bun: ">=1.4.0", zig: "0.16.0" },
      sourceDateEpoch: 1788868800,
      createdAt: "2026-09-08T12:00:00.000Z",
    };
    writeFileSync(join(runDir, "plan.json"), JSON.stringify(plan));

    const config = {
      schemaVersion: 1,
      repository: "/Users/indo/code/project/orca-lite",
      ghosttyRepository: "/Users/indo/code/project/orca-lite/src-tauri/vendor/ghostty",
      repo: "Indosaram/ferryx",
      hosts: {
        macbook: {
          platform: "darwin",
          ssh: null,
          root: hostRoot,
          minFreeBytes: 1024,
        },
      },
    };

    // Custom runner fixture simulating isolated platform build
    const mockRunner = async ({ hostName, workspaceDir, artifactsOutDir }) => {
      assert.equal(hostName, "macbook");
      assert.ok(existsSync(workspaceDir));
      // Simulate build artifact output
      const dummyApp = join(artifactsOutDir, "Ferryx.app.tar.gz");
      const dummySig = join(artifactsOutDir, "Ferryx.app.tar.gz.sig");
      const dummyDmg = join(artifactsOutDir, "Ferryx_universal.dmg");
      writeFileSync(dummyApp, "MOCK_APP_CONTENT");
      writeFileSync(dummySig, "untrusted comment: mock sig\nMOCK_SIGNATURE\n");
      writeFileSync(dummyDmg, "MOCK_DMG_CONTENT");

      return {
        exitCode: 0,
        toolchains: {
          node: "22.22.3",
          bun: "1.4.0",
          zig: "0.16.0",
          rust: "1.92.0",
          tauri: "2.10.1",
        },
      };
    };

    const receipt = await buildHost({
      hostName: "macbook",
      config,
      runDir,
      runner: mockRunner,
    });

    assert.equal(receipt.host, "macbook");
    assert.equal(receipt.exitCode, 0);
    assert.equal(receipt.artifacts.length, 2);

    const receiptPath = join(runDir, "receipts", "build-receipt-macbook.json");
    assert.ok(existsSync(receiptPath));
    const savedReceipt = JSON.parse(readFileSync(receiptPath, "utf8"));
    const validated = parseReceipt(savedReceipt, plan);
    assert.equal(validated.runId, plan.runId);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("createLinuxBuildScript: executes end-to-end against isolated git bundles and produces raw AppImage", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-test-linux-script-"));
  try {
    const repoA = join(tmp, "source-repo");
    mkdirSync(repoA);
    execFileSync("git", ["init"], { cwd: repoA });
    execFileSync("git", ["config", "user.name", "Test"], { cwd: repoA });
    execFileSync("git", ["config", "user.email", "test@test.com"], { cwd: repoA });

    const repoB = join(tmp, "ghostty-repo");
    mkdirSync(repoB);
    execFileSync("git", ["init"], { cwd: repoB });
    execFileSync("git", ["config", "user.name", "Test"], { cwd: repoB });
    execFileSync("git", ["config", "user.email", "test@test.com"], { cwd: repoB });
    writeFileSync(join(repoB, "ghostty.txt"), "ghostty");
    execFileSync("git", ["add", "ghostty.txt"], { cwd: repoB });
    execFileSync("git", ["commit", "-m", "ghostty pin"], { cwd: repoB });
    const ghosttySha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoB, encoding: "utf8" }).trim();

    // Create expected files in source-repo
    mkdirSync(join(repoA, "src-tauri", "native_terminal"), { recursive: true });
    writeFileSync(
      join(repoA, "src-tauri", "native_terminal", "build_ghostty.rs"),
      `pub const EXPECTED_GHOSTTY_SHA: &str = "${ghosttySha}";\n`,
    );
    mkdirSync(join(repoA, "scripts"));
    writeFileSync(
      join(repoA, "scripts", "sync-version.mjs"),
      "console.log('stamped');\n",
    );
    mkdirSync(join(repoA, "ui"));
    writeFileSync(join(repoA, "ui", "package.json"), "{}");
    execFileSync("git", ["add", "."], { cwd: repoA });
    execFileSync("git", ["commit", "-m", "init"], { cwd: repoA });
    const sourceSha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoA, encoding: "utf8" }).trim();

    const workspaceDir = join(tmp, "workspace");
    mkdirSync(workspaceDir);

    createGitBundles({
      repoDir: repoA,
      ghosttyRepoDir: repoB,
      commitSha: sourceSha,
      ghosttyPin: ghosttySha,
      outDir: workspaceDir,
    });

    // Mock bin directory
    const binDir = join(tmp, "bin");
    mkdirSync(binDir);

    // Mock bun
    const bunScript = `#!/usr/bin/env bash
if [ "$1" = "--version" ]; then
  echo "1.4.0"
elif [ "$1" = "install" ]; then
  exit 0
elif [ "$1" = "tauri" ] && [ "$2" = "build" ]; then
  mkdir -p "$CARGO_TARGET_DIR/release/bundle/appimage"
  mkdir -p "$CARGO_TARGET_DIR/release/bundle/deb"
  printf "ELF_MOCK_APPIMAGE_BYTES" > "$CARGO_TARGET_DIR/release/bundle/appimage/Ferryx_1.0.0_amd64.AppImage"
  printf "DEB_MOCK_BYTES" > "$CARGO_TARGET_DIR/release/bundle/deb/Ferryx_1.0.0_amd64.deb"
  exit 0
fi
`;
    writeFileSync(join(binDir, "bun"), bunScript);
    chmodSync(join(binDir, "bun"), 0o755);

    // Mock zig
    writeFileSync(join(binDir, "zig"), "#!/usr/bin/env bash\necho '0.16.0'\n");
    chmodSync(join(binDir, "zig"), 0o755);

    // Mock rustc
    writeFileSync(join(binDir, "rustc"), "#!/usr/bin/env bash\necho 'rustc 1.92.0 (mock)'\n");
    chmodSync(join(binDir, "rustc"), 0o755);

    const plan = {
      commitSha: sourceSha,
      ghosttyPin: ghosttySha,
      tag: "v2026.09.08.1",
      sourceDateEpoch: 1788868800,
    };

    const script = createLinuxBuildScript({
      workspaceDir,
      plan,
      hostConfig: { path: binDir },
    });

    const runRes = execFileSync("bash", ["-s"], {
      input: script,
      encoding: "utf8",
    });

    const appImageFile = join(workspaceDir, "out", "Ferryx_amd64.AppImage");
    const debFile = join(workspaceDir, "out", "Ferryx_amd64.deb");
    const tarGzFile = join(workspaceDir, "out", "Ferryx_amd64.AppImage.tar.gz");

    assert.ok(existsSync(appImageFile), "Raw AppImage must exist in output directory");
    assert.equal(readFileSync(appImageFile, "utf8"), "ELF_MOCK_APPIMAGE_BYTES");
    assert.ok(existsSync(debFile), "Deb package must exist in output directory");
    assert.equal(readFileSync(debFile, "utf8"), "DEB_MOCK_BYTES");
    assert.equal(existsSync(tarGzFile), false, "AppImage.tar.gz must NOT be created");
    assert.match(runRes, /---BUILD_RESULT---/);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

/**
 * Builds a fixture .app tree plus a stale bundler DMG, and a fake exec that
 * models the real commands (codesign mutates file contents, ditto copies,
 * hdiutil snapshots the staging folder) so DMG contents can be inspected.
 */
function createMacFinalizeFixture(tmp, { notaryStatus = "Accepted" } = {}) {
  const macosDir = join(tmp, "bundle", "macos");
  const appPath = join(macosDir, "Ferryx.app");
  const helperDir = join(appPath, "Contents", "Resources", "helpers");
  mkdirSync(join(appPath, "Contents", "MacOS"), { recursive: true });
  mkdirSync(join(helperDir, "aarch64-apple-darwin"), { recursive: true });
  mkdirSync(join(helperDir, "x86_64-apple-darwin"), { recursive: true });
  writeFileSync(join(appPath, "Contents", "MacOS", "ferryx"), "macho:main");
  writeFileSync(join(helperDir, "aarch64-apple-darwin", "ferryx-remote-helper"), "macho:helper-arm");
  writeFileSync(join(helperDir, "x86_64-apple-darwin", "ferryx-remote-helper"), "macho:helper-x86");

  const dmgDir = join(tmp, "bundle", "dmg");
  mkdirSync(dmgDir, { recursive: true });
  const dmgPath = join(dmgDir, "Ferryx_2026.920.3_universal.dmg");
  // Simulates the Tauri bundler's DMG: a snapshot of the app taken before the
  // coordinator's re-sign pass, so its helpers are unsigned.
  writeFileSync(dmgPath, JSON.stringify(snapshotTree(appPath)));

  const workspaceDir = join(tmp, "workspace");
  mkdirSync(workspaceDir, { recursive: true });

  const calls = [];
  const exec = (command, args) => {
    calls.push([command, ...args].join(" "));
    if (command === "find") {
      return { status: 0, stdout: listFiles(args[0]).join("\n") + "\n", stderr: "" };
    }
    if (command === "file") {
      return { status: 0, stdout: "Mach-O 64-bit executable\n", stderr: "" };
    }
    if (command === "codesign") {
      const target = args[args.length - 1];
      // Model codesign mutating the Mach-O it seals, so a DMG built from a
      // pre-signing copy of the app is distinguishable from one built after.
      // Sealing the DMG itself leaves its payload readable for assertions.
      if (
        args[0] === "--force" &&
        !target.endsWith(".dmg") &&
        existsSync(target) &&
        !statSync(target).isDirectory()
      ) {
        writeFileSync(target, `${readFileSync(target, "utf8")}+signed`);
      }
      return { status: 0, stdout: "", stderr: "Authority=Developer ID Application: Indo Yoon (5DUM8WPB4C)\n" };
    }
    if (command === "rm") {
      rmSync(args[args.length - 1], { recursive: true, force: true });
      return { status: 0, stdout: "", stderr: "" };
    }
    if (command === "mkdir") {
      mkdirSync(args[args.length - 1], { recursive: true });
      return { status: 0, stdout: "", stderr: "" };
    }
    if (command === "ln") {
      writeFileSync(args[args.length - 1], "symlink:/Applications");
      return { status: 0, stdout: "", stderr: "" };
    }
    if (command === "ditto") {
      const src = args[args.length - 2];
      const dst = args[args.length - 1];
      if (args.includes("-c")) {
        writeFileSync(dst, JSON.stringify(snapshotTree(src)));
      } else {
        cpSync(src, dst, { recursive: true });
      }
      return { status: 0, stdout: "", stderr: "" };
    }
    if (command === "hdiutil") {
      const srcFolder = args[args.indexOf("-srcfolder") + 1];
      writeFileSync(args[args.length - 1], JSON.stringify(snapshotTree(srcFolder)));
      return { status: 0, stdout: "", stderr: "" };
    }
    if (command === "xcrun" && args[0] === "notarytool") {
      return {
        status: 0,
        stdout: JSON.stringify({ id: "87be4dc7-95e6-460a-a8bf-7e9e4360935d", status: notaryStatus, message: "Processing complete" }),
        stderr: "",
      };
    }
    if (command === "spctl") {
      return { status: 0, stdout: "", stderr: "source=Notarized Developer ID\n" };
    }
    return { status: 0, stdout: "", stderr: "" };
  };

  return { appPath, dmgPath, workspaceDir, calls, exec };
}

function listFiles(root) {
  const out = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else out.push(full);
    }
  };
  walk(root);
  return out;
}

function snapshotTree(root) {
  const snapshot = {};
  for (const file of listFiles(root)) {
    snapshot[file.slice(root.length + 1)] = readFileSync(file, "utf8");
  }
  return snapshot;
}

test("finalizeMacosBundle: DMG is rebuilt from the signed+stapled app, so helpers inside it are signed", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-dmg-order-"));
  try {
    const fixture = createMacFinalizeFixture(tmp);
    const staleDmg = JSON.parse(readFileSync(fixture.dmgPath, "utf8"));
    assert.equal(
      staleDmg["Contents/Resources/helpers/aarch64-apple-darwin/ferryx-remote-helper"],
      "macho:helper-arm",
      "fixture precondition: bundler DMG starts with unsigned helpers",
    );

    finalizeMacosBundle({
      appPath: fixture.appPath,
      dmgPath: fixture.dmgPath,
      workspaceDir: fixture.workspaceDir,
      signingIdentity: "Developer ID Application: Indo Yoon (5DUM8WPB4C)",
      notaryProfile: "FerryxNotary",
      approveNotarization: true,
      exec: fixture.exec,
    });

    const shippedDmg = JSON.parse(readFileSync(fixture.dmgPath, "utf8"));
    for (const arch of ["aarch64-apple-darwin", "x86_64-apple-darwin"]) {
      const key = `Ferryx.app/Contents/Resources/helpers/${arch}/ferryx-remote-helper`;
      assert.ok(key in shippedDmg, `DMG must contain the embedded helper for ${arch}`);
      assert.match(
        shippedDmg[key],
        /\+signed$/,
        `helper for ${arch} inside the DMG must come from the re-signed app`,
      );
    }
    assert.match(shippedDmg["Ferryx.app/Contents/MacOS/ferryx"], /\+signed$/);
    assert.ok("Applications" in shippedDmg, "DMG must keep the /Applications drop target");
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("finalizeMacosBundle: orders DMG creation after app staple and before DMG notarization", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-dmg-sequence-"));
  try {
    const fixture = createMacFinalizeFixture(tmp);
    finalizeMacosBundle({
      appPath: fixture.appPath,
      dmgPath: fixture.dmgPath,
      workspaceDir: fixture.workspaceDir,
      signingIdentity: "Developer ID Application: Indo Yoon (5DUM8WPB4C)",
      notaryProfile: "FerryxNotary",
      approveNotarization: true,
      exec: fixture.exec,
    });

    const calls = fixture.calls;
    const indexOf = (predicate, label) => {
      const idx = calls.findIndex(predicate);
      assert.notEqual(idx, -1, `expected command not issued: ${label}`);
      return idx;
    };

    const helperSign = indexOf(
      (c) => c.startsWith("codesign --force") && c.includes("--timestamp") && c.includes("ferryx-remote-helper"),
      "codesign of embedded helper with --timestamp",
    );
    const appStaple = indexOf((c) => c.startsWith("xcrun stapler staple") && c.endsWith(".app"), "stapler staple app");
    const dmgCreate = indexOf((c) => c.startsWith("hdiutil create"), "hdiutil create for DMG");
    const dmgSubmit = indexOf(
      (c) => c.startsWith("xcrun notarytool submit") && c.includes(".dmg"),
      "notarytool submit for DMG",
    );
    const dmgStaple = indexOf((c) => c.startsWith("xcrun stapler staple") && c.includes(".dmg"), "stapler staple DMG");

    assert.ok(helperSign < appStaple, "helpers must be re-signed before the app is stapled");
    assert.ok(appStaple < dmgCreate, "DMG must be created after the app is notarized and stapled");
    assert.ok(dmgCreate < dmgSubmit, "DMG must be recreated before it is submitted for notarization");
    assert.ok(dmgSubmit < dmgStaple, "DMG staple must follow its notarization");

    const dittoIntoStaging = indexOf(
      (c) => c.startsWith("ditto ") && !c.includes(" -c ") && c.includes("dmg-staging"),
      "ditto of final app into DMG staging",
    );
    assert.ok(appStaple < dittoIntoStaging, "staging copy must be taken from the stapled app");
    assert.ok(dittoIntoStaging < dmgCreate, "staging copy must precede hdiutil create");
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("submitForNotarization: fails closed on an Invalid submission instead of proceeding to staple", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-notary-invalid-"));
  try {
    const fixture = createMacFinalizeFixture(tmp, { notaryStatus: "Invalid" });
    assert.throws(
      () =>
        finalizeMacosBundle({
          appPath: fixture.appPath,
          dmgPath: fixture.dmgPath,
          workspaceDir: fixture.workspaceDir,
          signingIdentity: "Developer ID Application: Indo Yoon (5DUM8WPB4C)",
          notaryProfile: "FerryxNotary",
          approveNotarization: true,
          exec: fixture.exec,
        }),
      /Notarization status 'Invalid'.*87be4dc7-95e6-460a-a8bf-7e9e4360935d/s,
    );
    assert.equal(
      fixture.calls.some((c) => c.startsWith("xcrun stapler staple")),
      false,
      "no artifact may be stapled after an Invalid notarization result",
    );
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("submitForNotarization: returns the parsed Accepted result and requests JSON output", () => {
  const calls = [];
  const result = submitForNotarization({
    path: "/tmp/Ferryx-notary.zip",
    notaryProfile: "FerryxNotary",
    exec: (command, args) => {
      calls.push([command, ...args].join(" "));
      return {
        status: 0,
        stdout: `Submission ID received\n${JSON.stringify({ id: "abc", status: "Accepted" })}\n`,
        stderr: "",
      };
    },
  });
  assert.equal(result.status, "Accepted");
  assert.equal(result.id, "abc");
  assert.match(calls[0], /--output-format json/);
  assert.match(calls[0], /--wait/);
});

test("recreateDmgFromApp: replaces the existing image with one built from the given app", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-dmg-recreate-"));
  try {
    const fixture = createMacFinalizeFixture(tmp);
    writeFileSync(join(fixture.appPath, "Contents", "MacOS", "ferryx"), "macho:main+signed");
    const stagingDir = join(fixture.workspaceDir, "dmg-staging");

    const { volumeName } = recreateDmgFromApp({
      appPath: fixture.appPath,
      dmgPath: fixture.dmgPath,
      stagingDir,
      exec: fixture.exec,
    });

    assert.equal(volumeName, "Ferryx");
    const snapshot = JSON.parse(readFileSync(fixture.dmgPath, "utf8"));
    assert.equal(snapshot["Ferryx.app/Contents/MacOS/ferryx"], "macho:main+signed");
    assert.equal(existsSync(stagingDir), false, "staging directory must be cleaned up");
    assert.match(
      fixture.calls.find((c) => c.startsWith("hdiutil create")),
      /-volname Ferryx .*-srcfolder .*dmg-staging/,
    );
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("signUpdaterArtifact: fails closed when signing keys are absent from environment", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-test-sign-fail-"));
  try {
    const dummyFile = join(tmp, "artifact.AppImage");
    writeFileSync(dummyFile, "MOCK_APPIMAGE");

    const savedKey = process.env.TAURI_SIGNING_PRIVATE_KEY;
    const savedPath = process.env.TAURI_SIGNING_PRIVATE_KEY_PATH;
    delete process.env.TAURI_SIGNING_PRIVATE_KEY;
    delete process.env.TAURI_SIGNING_PRIVATE_KEY_PATH;

    try {
      await assert.rejects(
        () => signUpdaterArtifact({ artifactPath: dummyFile, repoDir: tmp }),
        /TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH required/,
      );
    } finally {
      if (savedKey !== undefined) process.env.TAURI_SIGNING_PRIVATE_KEY = savedKey;
      if (savedPath !== undefined) process.env.TAURI_SIGNING_PRIVATE_KEY_PATH = savedPath;
    }
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("signUpdaterArtifact: signs artifact and validates signature against updater public key", async () => {
  function createMinisignTestKeys() {
    const { publicKey, privateKey } = generateKeyPairSync("ed25519");
    const rawPub = publicKey.export({ format: "der", type: "spki" }).subarray(12);
    const keyId = randomBytes(8);
    const keyIdHex = Buffer.from(keyId).reverse().toString("hex").toUpperCase();

    const pubBin = Buffer.concat([Buffer.from("Ed", "utf8"), keyId, rawPub]);
    const pubB64 = pubBin.toString("base64");
    const pubText = `untrusted comment: minisign public key ${keyIdHex}\n${pubB64}\n`;
    const pubWrapped = Buffer.from(pubText, "utf8").toString("base64");

    function signPayload(data, comment = "test trusted comment") {
      const dataBuf = Buffer.isBuffer(data) ? data : Buffer.from(data);
      const blakeHash = createHash("blake2b512").update(dataBuf).digest();
      const payloadSig = sign(null, blakeHash, privateKey);
      const sigBin = Buffer.concat([Buffer.from("ED", "utf8"), keyId, payloadSig]);
      const sigB64 = sigBin.toString("base64");

      const globalMsg = Buffer.concat([payloadSig, Buffer.from(comment, "utf8")]);
      const globalSig = sign(null, globalMsg, privateKey);
      const globalSigB64 = globalSig.toString("base64");

      return [
        "untrusted comment: signature from test key",
        sigB64,
        `trusted comment: ${comment}`,
        globalSigB64,
        "",
      ].join("\n");
    }

    return { pubWrapped, signPayload };
  }

  const tmp = mkdtempSync(join(tmpdir(), "ferryx-test-signer-"));
  try {
    mkdirSync(join(tmp, "src-tauri"), { recursive: true });
    const keys = createMinisignTestKeys();
    writeFileSync(
      join(tmp, "src-tauri", "tauri.conf.json"),
      JSON.stringify({ plugins: { updater: { pubkey: keys.pubWrapped } } }),
    );

    const artifactFile = join(tmp, "Ferryx_amd64.AppImage");
    writeFileSync(artifactFile, "REAL_APPIMAGE_BYTES");

    const mockSigner = join(tmp, "mock-tauri.mjs");
    writeFileSync(
      mockSigner,
      `#!/usr/bin/env node
import { writeFileSync, readFileSync } from "node:fs";
const target = process.argv[process.argv.length - 1];
const data = readFileSync(target);
const sig = ${JSON.stringify(keys.signPayload(Buffer.from("REAL_APPIMAGE_BYTES")))};
writeFileSync(target + ".sig", sig);
`,
    );
    chmodSync(mockSigner, 0o755);

    process.env.TAURI_SIGNING_PRIVATE_KEY = "MOCK_KEY";
    process.env.FERRYX_TAURI_BIN = mockSigner;

    await signUpdaterArtifact({
      artifactPath: artifactFile,
      repoDir: tmp,
    });

    const sigFile = `${artifactFile}.sig`;
    assert.ok(existsSync(sigFile), "Signature file must be created alongside artifact");
  } finally {
    delete process.env.TAURI_SIGNING_PRIVATE_KEY;
    delete process.env.FERRYX_TAURI_BIN;
    rmSync(tmp, { recursive: true, force: true });
  }
});

