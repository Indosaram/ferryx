import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync, mkdirSync, existsSync, readFileSync, chmodSync, readdirSync, statSync, cpSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { execFileSync, spawn } from "node:child_process";
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
  measureMacosFloors,
  parseFloorResult,
} from "./lib/release-platforms.mjs";
import { evaluateFloors, parseReceipt } from "./lib/release-contract.mjs";

// Floor measurements of a conforming macOS build (notarized; x86_64 minos at or below 10.15,
// arm64 at the 11.0 Apple Silicon floor).
const NOTARIZED = (target) => ({ target, spctlStatus: 0, spctl: `${target}: accepted\nsource=Notarized Developer ID`, staplerStatus: 0 });
const AD_HOC = (target) => ({ target, spctlStatus: 3, spctl: `${target}: rejected\nsource=no usable signature`, staplerStatus: 65 });
const MAC_FLOORS = {
  "macos-updater": { minos: { x86_64: "10.15", arm64: "11.0" }, notarization: [NOTARIZED("app")] },
  dmg: { minos: { x86_64: "10.15", arm64: "11.0" }, notarization: [NOTARIZED("app"), NOTARIZED("dmg")] },
  "cli-darwin-universal": { minos: { x86_64: "10.13", arm64: "11.0" } },
};

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

test("createWindowsBuildScript: reads signing secret from a remote file", () => {
  const script = createWindowsBuildScript({
    workspaceDir: "C:/Users/sook/ferryx-releases/test",
    plan: {
      tag: "v2026.09.20.7",
      msixVersion: "2026.920.7.0",
      channels: { nsisMigration: true },
    },
    hostConfig: { root: "C:/Users/sook/ferryx-releases" },
    signingSecretFile: "tauri-signing-secret.json",
  });
  assert.match(script, /ConvertFrom-Json/);
  assert.match(script, /Remove-Item -LiteralPath \$signingSecretFile/);
  assert.doesNotMatch(script, /TAURI_SIGNING_PRIVATE_KEY\s*=\s*['\"]/);
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

test("createGitBundles: concurrent invocations with platform-scoped outDirs do not collide on temp refs or locks", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-bundle-concurrency-test-"));
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

    const platforms = ["macbook", "omaki", "maho-win"];
    const libPath = resolve("scripts/lib/release-platforms.mjs");
    const workerScript = `
      import { createGitBundles } from ${JSON.stringify(libPath)};
      const [repoDir, ghosttyRepoDir, commitSha, ghosttyPin, outDir] = process.argv.slice(1);
      createGitBundles({ repoDir, ghosttyRepoDir, commitSha, ghosttyPin, outDir });
    `;

    const activeChildren = [];
    const workerPromises = platforms.map((platform) => {
      const outDir = join(tmp, "bundles", platform);
      return new Promise((resolveWorker, rejectWorker) => {
        const child = spawn(process.execPath, [
          "--input-type=module",
          "-e", workerScript,
          repoA, repoB, shaA, shaB, outDir,
        ], { stdio: ["ignore", "pipe", "pipe"] });

        activeChildren.push(child);

        let stderr = "";
        child.stderr.on("data", (chunk) => { stderr += chunk; });

        let timedOut = false;
        const timer = setTimeout(() => {
          timedOut = true;
          child.kill("SIGKILL");
        }, 30000);

        child.on("error", (err) => {
          clearTimeout(timer);
          rejectWorker(err);
        });

        child.on("exit", (code, signal) => {
          clearTimeout(timer);
          if (timedOut) {
            rejectWorker(new Error(`Worker for ${platform} timed out after 30000ms: ${stderr}`));
          } else if (code === 0) {
            resolveWorker({ platform, outDir });
          } else {
            rejectWorker(new Error(`Worker for ${platform} exited with code ${code} (${signal}): ${stderr}`));
          }
        });
      });
    });

    const results = await Promise.allSettled(workerPromises);
    const failures = results.filter((r) => r.status === "rejected");
    if (failures.length > 0) {
      for (const child of activeChildren) {
        try { child.kill("SIGKILL"); } catch {}
      }
      throw new Error(`Concurrent bundle creation failed: ${failures.map((f) => f.reason.message).join("; ")}`);
    }
    const finishedWorkers = results.map((r) => r.value);

    // Assert each platform bundle was created and independently clones matching commit and pin
    for (const { platform, outDir } of finishedWorkers) {
      const sourceBundle = join(outDir, "source.bundle");
      const ghosttyBundle = join(outDir, "ghostty.bundle");
      assert.ok(existsSync(sourceBundle));
      assert.ok(existsSync(ghosttyBundle));

      const cloneSource = join(tmp, `clone-source-${platform}`);
      execFileSync("git", ["clone", sourceBundle, cloneSource], { stdio: "pipe" });
      const clonedSourceSha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: cloneSource, encoding: "utf8" }).trim();
      assert.equal(clonedSourceSha, shaA);

      const cloneGhostty = join(tmp, `clone-ghostty-${platform}`);
      execFileSync("git", ["clone", ghosttyBundle, cloneGhostty], { stdio: "pipe" });
      const clonedGhosttySha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: cloneGhostty, encoding: "utf8" }).trim();
      assert.equal(clonedGhosttySha, shaB);
    }
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("buildHost: reuses existing host-scoped bundles when present", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-buildhost-scoped-bundles-"));
  try {
    const runDir = join(tmp, "run");
    mkdirSync(runDir);
    mkdirSync(join(runDir, "artifacts"));
    mkdirSync(join(runDir, "artifacts", "linux"));
    mkdirSync(join(runDir, "receipts"));
    mkdirSync(join(runDir, "bundles"));
    const hostRoot = join(tmp, "host-root");
    mkdirSync(hostRoot);

    // Pre-create host-scoped bundles for macbook, leave shared bundles absent
    const macbookBundlesDir = join(runDir, "bundles", "macbook");
    mkdirSync(macbookBundlesDir, { recursive: true });
    writeFileSync(join(macbookBundlesDir, "source.bundle"), "HOST_SCOPED_SOURCE_BUNDLE");
    writeFileSync(join(macbookBundlesDir, "ghostty.bundle"), "HOST_SCOPED_GHOSTTY_BUNDLE");

    const plan = {
      schemaVersion: 1,
      runId: "rel-run-fixture-host-scoped",
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

    let observedHost = null;
    const mockRunner = async ({ hostName, workspaceDir, artifactsOutDir }) => {
      observedHost = hostName;
      const dummyApp = join(artifactsOutDir, "Ferryx.app.tar.gz");
      const dummySig = join(artifactsOutDir, "Ferryx.app.tar.gz.sig");
      const dummyDmg = join(artifactsOutDir, "Ferryx_universal.dmg");
      writeFileSync(dummyApp, "MOCK_APP_CONTENT");
      writeFileSync(dummySig, "untrusted comment: mock sig\nMOCK_SIGNATURE\n");
      writeFileSync(dummyDmg, "MOCK_DMG_CONTENT");
      writeFileSync(join(artifactsOutDir, "ferryx-cli-darwin-universal"), "MOCK_CLI_CONTENT");
      return {
        exitCode: 0,
        toolchains: { node: "22.22.3", bun: "1.4.0", zig: "0.16.0", rust: "1.92.0" },
        floors: MAC_FLOORS,
      };
    };

    const receipt = await buildHost({
      hostName: "macbook",
      config,
      runDir,
      runner: mockRunner,
      macosMinimum: "10.15",
    });

    assert.equal(receipt.host, "macbook");
    assert.equal(observedHost, "macbook");
    // Verify shared bundles were never created, existing host-scoped bundles were reused
    assert.ok(!existsSync(join(runDir, "bundles", "source.bundle")));
    assert.ok(existsSync(join(macbookBundlesDir, "source.bundle")));
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("buildHost: creates host-scoped bundles into platform subdirectory when shared bundles are absent", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-buildhost-create-scoped-"));
  try {
    const repoA = join(tmp, "source-repo");
    mkdirSync(repoA);
    execFileSync("git", ["init"], { cwd: repoA });
    execFileSync("git", ["config", "user.name", "Test"], { cwd: repoA });
    execFileSync("git", ["config", "user.email", "test@test.com"], { cwd: repoA });
    mkdirSync(join(repoA, "src-tauri", "native_terminal"), { recursive: true });

    const repoB = join(tmp, "ghostty-repo");
    mkdirSync(repoB);
    execFileSync("git", ["init"], { cwd: repoB });
    execFileSync("git", ["config", "user.name", "Test"], { cwd: repoB });
    execFileSync("git", ["config", "user.email", "test@test.com"], { cwd: repoB });
    writeFileSync(join(repoB, "ghostty.txt"), "GhosttyVT");
    execFileSync("git", ["add", "ghostty.txt"], { cwd: repoB });
    execFileSync("git", ["commit", "-m", "ghostty"], { cwd: repoB });
    const ghosttySha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoB, encoding: "utf8" }).trim();

    writeFileSync(
      join(repoA, "src-tauri", "native_terminal", "build_ghostty.rs"),
      `pub const EXPECTED_GHOSTTY_SHA: &str = "${ghosttySha}";\n`,
    );
    execFileSync("git", ["add", "."], { cwd: repoA });
    execFileSync("git", ["commit", "-m", "init source"], { cwd: repoA });
    const sourceSha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoA, encoding: "utf8" }).trim();

    const runDir = join(tmp, "run");
    mkdirSync(runDir);
    mkdirSync(join(runDir, "artifacts"));
    mkdirSync(join(runDir, "artifacts", "darwin"));
    mkdirSync(join(runDir, "receipts"));
    mkdirSync(join(runDir, "bundles"));
    const hostRoot = join(tmp, "host-root");
    mkdirSync(hostRoot);

    const plan = {
      schemaVersion: 1,
      runId: "rel-run-create-scoped-1",
      repo: "Indosaram/ferryx",
      commitSha: sourceSha,
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
      repository: repoA,
      ghosttyRepository: repoB,
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

    let observedHost = null;
    const mockRunner = async ({ hostName, workspaceDir, artifactsOutDir }) => {
      observedHost = hostName;
      const dummyApp = join(artifactsOutDir, "Ferryx.app.tar.gz");
      const dummySig = join(artifactsOutDir, "Ferryx.app.tar.gz.sig");
      const dummyDmg = join(artifactsOutDir, "Ferryx_universal.dmg");
      writeFileSync(dummyApp, "MOCK_APP_CONTENT");
      writeFileSync(dummySig, "untrusted comment: mock sig\nMOCK_SIGNATURE\n");
      writeFileSync(dummyDmg, "MOCK_DMG_CONTENT");
      writeFileSync(join(artifactsOutDir, "ferryx-cli-darwin-universal"), "MOCK_CLI_CONTENT");
      return {
        exitCode: 0,
        toolchains: { node: "22.22.3", bun: "1.4.0", zig: "0.16.0", rust: "1.92.0" },
        floors: MAC_FLOORS,
      };
    };

    const receipt = await buildHost({
      hostName: "macbook",
      config,
      runDir,
      runner: mockRunner,
      macosMinimum: "10.15",
    });

    assert.equal(receipt.host, "macbook");
    assert.equal(observedHost, "macbook");

    // Verify host-scoped bundle was created and shared bundle was NOT written
    const hostSourceBundle = join(runDir, "bundles", "macbook", "source.bundle");
    const hostGhosttyBundle = join(runDir, "bundles", "macbook", "ghostty.bundle");
    assert.ok(existsSync(hostSourceBundle), "host-scoped source.bundle should be created");
    assert.ok(existsSync(hostGhosttyBundle), "host-scoped ghostty.bundle should be created");
    assert.ok(!existsSync(join(runDir, "bundles", "source.bundle")), "shared source.bundle should not be created");

    // Verify created bundle is valid and matches commitSha
    const testClone = join(tmp, "verify-clone");
    execFileSync("git", ["clone", hostSourceBundle, testClone], { stdio: "pipe" });
    const clonedSha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: testClone, encoding: "utf8" }).trim();
    assert.equal(clonedSha, sourceSha);
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
  // The standalone CLI links against the glibc 2.17 floor; the desktop bundles are built in an
  // Ubuntu 22.04 (glibc 2.35, webkit2gtk-4.1) container.
  assert.match(linux, /cargo zigbuild .*--no-default-features --bin ferryx-cli --target x86_64-unknown-linux-gnu\.2\.17/);
  assert.match(linux, /docker build .*packaging\/linux\/bundle\.Dockerfile/);
  assert.match(linux, /docker run --rm [\s\S]*"\$bundle_image" bun tauri build --bundles appimage,deb/);
  assert.doesNotMatch(linux, /^bun tauri build/m);
  assert.match(linux, /linuxBuilder/);
  // Floor gate input: GLIBC_ maxima measured in the bundle image over every ELF of each artifact.
  assert.match(linux, /objdump -T/);
  assert.match(linux, /--appimage-extract/);
  assert.match(linux, /dpkg-deb -x "\$OUT_DIR\/Ferryx_amd64\.deb"/);
  assert.match(linux, /-v "\$out_dir:\$out_dir:ro" .*"\$bundle_image" bash "\$floor_dir\/scan\.sh"/s);
  assert.match(linux, /---FLOOR_RESULT---/);
  assert.ok(linux.indexOf("---FLOOR_RESULT---") < linux.indexOf("---BUILD_RESULT---"));

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
  assert.match(windows, /--bundles nsis -c '{"build":{"beforeBuildCommand":""}}'; if \(\$LASTEXITCODE -ne 0\) \{ exit \$LASTEXITCODE \}/);
  assert.match(windows, /cargo build --manifest-path \(Join-Path \$sourceDir 'src-tauri\/Cargo\.toml'\) --release --no-default-features --bin ferryx-cli\n  \$cliBuildExit = \$LASTEXITCODE\n/);
  assert.match(windows, /if \(\$cliBuildExit -ne 0\) \{ exit \$cliBuildExit \}/);
  // Only the CLI build links the CRT statically (it skips tauri-build's static VC runtime): RUSTFLAGS
  // gains +crt-static right before it and is restored before the NSIS desktop build.
  assert.equal((windows.match(/target-feature=\+crt-static/g) ?? []).length, 1);
  const setFlags = windows.indexOf("$env:RUSTFLAGS = (@($prevRustflags, '-C target-feature=+crt-static') | Where-Object { $_ }) -join ' '");
  const cliBuild = windows.indexOf("--bin ferryx-cli\n");
  const restore = windows.indexOf("} finally {\n  if ($null -eq $prevRustflags) { Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue } else { $env:RUSTFLAGS = $prevRustflags }\n}");
  assert.ok(setFlags > windows.indexOf("bun tauri build --no-bundle"), "desktop build runs without the CLI flags");
  assert.ok(setFlags < cliBuild && cliBuild < restore, "+crt-static is scoped to the CLI cargo build");
  assert.ok(restore < windows.indexOf("--bundles nsis"), "RUSTFLAGS is restored before the NSIS build");
  assert.doesNotMatch(linux, /crt-static/);
  assert.match(windows, /Copy-Item -LiteralPath \$cliExe -Destination \(Join-Path \$outDir 'ferryx-cli-windows-amd64\.exe'\)/);
  // The CLI path keeps its backslash (a single one in the JS template became a form feed).
  assert.ok(windows.includes("$cliExe = Join-Path $cargoTarget 'release\\ferryx-cli.exe'"));
  assert.ok(!windows.includes("\f"), "generated PowerShell must not contain a form feed");
  // CARGO_ENCODED_RUSTFLAGS (even empty) would silently override RUSTFLAGS: refuse before any build.
  const encodedGuard = windows.indexOf("if (Test-Path Env:CARGO_ENCODED_RUSTFLAGS) { throw ");
  assert.ok(encodedGuard > 0 && encodedGuard < windows.indexOf("bun tauri build"), "CARGO_ENCODED_RUSTFLAGS guard runs before the first build");
  assert.equal((windows.match(/CARGO_ENCODED_RUSTFLAGS\)? \{ throw/g) ?? []).length, 1);
  assert.doesNotMatch(windows, /if \(\$env:CARGO_ENCODED_RUSTFLAGS\)/);
  const storeOnly = createWindowsBuildScript({
    workspaceDir: "C:/release root/run", plan: { ...plan, channels: { nsisMigration: false } },
    hostConfig: {},
  });
  // Floor gate input: dumpbin /dependents of the executables that actually ship. The NSIS build
  // rebuilds every bin in target\release, so the CLI is measured as its shipped copy and the MSIX
  // ferryx.exe as a copy taken before the NSIS rebuild.
  for (const script of [windows, storeOnly]) {
    assert.match(script, /& \$dumpbin \/nologo \/dependents \$Path/);
    assert.ok(script.includes("$cliDependents = Get-FerryxDependents (Join-Path $outDir 'ferryx-cli-windows-amd64.exe')"));
    assert.ok(script.includes("$appDependents = Get-FerryxDependents $msixAppExe\n"));
    assert.doesNotMatch(script, /Get-FerryxDependents \$cliExe/);
    assert.ok(script.indexOf("$msixAppExe = $executables[0].FullName\n") > script.indexOf("build-msix.ps1"));
    assert.match(script, /Write-Output '---FLOOR_RESULT---'/);
  }
  const msixCopy = windows.indexOf("$msixAppExe = Join-Path $workspace 'floor-msix-ferryx.exe'\nCopy-Item -LiteralPath $executables[0].FullName -Destination $msixAppExe\n");
  assert.ok(msixCopy > windows.indexOf("build-msix.ps1") && msixCopy < windows.indexOf("--bundles nsis"), "MSIX exe kept before the NSIS rebuild");
  assert.ok(windows.indexOf("$floors['nsis'] = @{ dependents = (Get-FerryxDependents $executables[0].FullName) }") > windows.indexOf("--bundles nsis"));
  assert.doesNotMatch(storeOnly, /\$floors\['nsis'\]/);
  assert.doesNotMatch(storeOnly, /floor-msix-ferryx\.exe/);
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
      writeFileSync(join(artifactsOutDir, "ferryx-cli-darwin-universal"), "MOCK_CLI_CONTENT");

      return {
        exitCode: 0,
        toolchains: {
          node: "22.22.3",
          bun: "1.4.0",
          zig: "0.16.0",
          rust: "1.92.0",
          tauri: "2.10.1",
        },
        floors: MAC_FLOORS,
      };
    };

    const receipt = await buildHost({
      hostName: "macbook",
      config,
      runDir,
      runner: mockRunner,
      macosMinimum: "10.15",
    });

    assert.equal(receipt.host, "macbook");
    assert.equal(receipt.exitCode, 0);
    assert.equal(receipt.artifacts.length, 3);

    const receiptPath = join(runDir, "receipts", "build-receipt-macbook.json");
    assert.ok(existsSync(receiptPath));
    const savedReceipt = JSON.parse(readFileSync(receiptPath, "utf8"));
    const validated = parseReceipt(savedReceipt, plan);
    assert.equal(validated.runId, plan.runId);
    // The floor gate results are recorded in the receipt: every macOS floor passed, no waiver.
    assert.equal(validated.floors.waiver, null);
    assert.deepEqual(
      validated.floors.checks.map((c) => `${c.kind}/${c.check}/${c.pass}`).sort(),
      [
        "cli-darwin-universal/macos-minos/true",
        "dmg/macos-minos/true",
        "dmg/notarization/true",
        "macos-updater/macos-minos/true",
        "macos-updater/notarization/true",
      ],
    );
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

    // Mock cargo
    const cargoScript = `#!/usr/bin/env bash
if [ "$1" = "--version" ]; then
  echo "cargo 1.92.0 (mock)"
elif [ "$1" = "tauri" ] && [ "$2" = "--version" ]; then
  echo "tauri-cli 2.10.1"
elif [ "$1" = "zigbuild" ]; then
  mkdir -p "$CARGO_TARGET_DIR/x86_64-unknown-linux-gnu/release"
  printf "%s\n" "$*" >> "$CARGO_TARGET_DIR/cargo-build-args.log"
  printf "ELF_MOCK_CLI_BYTES" > "$CARGO_TARGET_DIR/x86_64-unknown-linux-gnu/release/ferryx-cli"
  exit 0
fi
`;
    writeFileSync(join(binDir, "cargo"), cargoScript);
    chmodSync(join(binDir, "cargo"), 0o755);

    // Mock docker: the container bundle build writes into the CARGO_TARGET_DIR passed with -e.
    const dockerScript = `#!/usr/bin/env bash
printf "%s\n" "$*" >> "$(dirname "$0")/docker-args.log"
if [ "$1" = "image" ]; then
  echo "sha256:mockimage"
elif [ "$1" = "run" ] && [[ "$*" == *floor-scan/scan.sh ]]; then
  echo "---FLOOR_RESULT---"
  echo '{"cli-linux-amd64":{"glibc":"2.17"},"appimage":{"glibc":"2.35"},"deb":{"glibc":"2.35"}}'
elif [ "$1" = "run" ]; then
  for arg in "$@"; do
    case "$arg" in CARGO_TARGET_DIR=*) target="\${arg#CARGO_TARGET_DIR=}" ;; esac
  done
  mkdir -p "$target/release/bundle/appimage" "$target/release/bundle/deb"
  printf "ELF_MOCK_APPIMAGE_BYTES" > "$target/release/bundle/appimage/Ferryx_1.0.0_amd64.AppImage"
  printf "DEB_MOCK_BYTES" > "$target/release/bundle/deb/Ferryx_1.0.0_amd64.deb"
fi
exit 0
`;
    writeFileSync(join(binDir, "docker"), dockerScript);
    chmodSync(join(binDir, "docker"), 0o755);

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
    const cliFile = join(workspaceDir, "out", "ferryx-cli-linux-amd64");
    const tarGzFile = join(workspaceDir, "out", "Ferryx_amd64.AppImage.tar.gz");

    assert.ok(existsSync(appImageFile), "Raw AppImage must exist in output directory");
    assert.equal(readFileSync(appImageFile, "utf8"), "ELF_MOCK_APPIMAGE_BYTES");
    assert.ok(existsSync(debFile), "Deb package must exist in output directory");
    assert.equal(readFileSync(debFile, "utf8"), "DEB_MOCK_BYTES");
    assert.ok(existsSync(cliFile), "Standalone CLI binary must exist in output directory");
    assert.equal(readFileSync(cliFile, "utf8"), "ELF_MOCK_CLI_BYTES");
    assert.equal(existsSync(tarGzFile), false, "AppImage.tar.gz must NOT be created");
    // The standalone Linux CLI must build without the desktop feature, so it links no GTK/WebKit.
    const cargoBuildArgs = readFileSync(join(workspaceDir, "cargo-target", "cargo-build-args.log"), "utf8");
    assert.match(cargoBuildArgs, /zigbuild .*--release --no-default-features --bin ferryx-cli --target x86_64-unknown-linux-gnu\.2\.17/);
    const dockerArgs = readFileSync(join(binDir, "docker-args.log"), "utf8");
    assert.match(dockerArgs, /^run --rm .* bun tauri build --bundles appimage,deb$/m);
    assert.match(dockerArgs, /^run --rm .*:ro .* bash \S*\/floor-scan\/scan\.sh$/m);
    assert.deepEqual(parseFloorResult(runRes), {
      "cli-linux-amd64": { glibc: "2.17" },
      appimage: { glibc: "2.35" },
      deb: { glibc: "2.35" },
    });
    assert.equal(existsSync(join(workspaceDir, "floor-scan")), false, "floor scan scratch must be removed");
    assert.match(runRes, /---BUILD_RESULT---/);
    const buildResult = JSON.parse(runRes.split("---BUILD_RESULT---")[1].trim());
    assert.equal(buildResult.tauri, "2.10.1");
    assert.match(buildResult.linuxBuilder, /ubuntu-22\.04 glibc-2\.35 image=sha256:mockimage/);
    assert.match(buildResult.linuxBuilder, /x86_64-unknown-linux-gnu\.2\.17/);
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

test("finalizeMacosBundle: signs the nested daemon helper bundle before the outer app, both with the recorded entitlements", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-nested-helper-order-"));
  try {
    const fixture = createMacFinalizeFixture(tmp);
    const helperApp = join(fixture.appPath, "Contents", "Helpers", "Ferryx Daemon.app");
    mkdirSync(join(helperApp, "Contents", "MacOS"), { recursive: true });
    writeFileSync(join(helperApp, "Contents", "MacOS", "ferryx"), "macho:main");
    const recorded = "<plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>";
    const entitlements = join(fixture.workspaceDir, "outer-entitlements.plist");
    writeFileSync(entitlements, `${recorded}\n`);
    // codesign -d --entitlements reports what the outer app was signed with.
    const exec = (command, args, opts) => {
      if (command === "codesign" && args[0] === "-d" && args.includes("--entitlements")) {
        fixture.calls.push([command, ...args].join(" "));
        return { status: 0, stdout: recorded, stderr: "" };
      }
      return fixture.exec(command, args, opts);
    };

    finalizeMacosBundle({
      appPath: fixture.appPath,
      dmgPath: fixture.dmgPath,
      workspaceDir: fixture.workspaceDir,
      signingIdentity: "Developer ID Application: Indo Yoon (5DUM8WPB4C)",
      notaryProfile: "FerryxNotary",
      approveNotarization: true,
      entitlements,
      exec,
    });

    const calls = fixture.calls;
    const signs = calls.filter((c) => c.startsWith("codesign --force"));
    const looseHelperExe = signs.findIndex((c) => c.endsWith("Ferryx Daemon.app/Contents/MacOS/ferryx"));
    const nestedBundle = signs.findIndex((c) => c.endsWith("Contents/Helpers/Ferryx Daemon.app"));
    const outerApp = signs.findIndex((c) => c.endsWith(fixture.appPath));
    assert.notEqual(looseHelperExe, -1, "the helper's Mach-O is signed as a loose file first");
    assert.notEqual(nestedBundle, -1, "the nested helper bundle is signed as a bundle");
    assert.notEqual(outerApp, -1, "the outer app is signed");
    assert.ok(looseHelperExe < nestedBundle, "loose Mach-O files precede the nested bundle");
    assert.ok(nestedBundle < outerApp, "the nested bundle signature precedes the outer app signature");
    for (const idx of [nestedBundle, outerApp]) {
      assert.ok(signs[idx].includes(`--entitlements ${entitlements}`), `bundle signature carries the recorded entitlements: ${signs[idx]}`);
      assert.ok(signs[idx].includes("--timestamp"), "Developer ID signatures keep a secure timestamp");
      assert.ok(!signs[idx].includes("--deep"), "bundles are signed without --deep");
    }
    assert.ok(!signs[looseHelperExe].includes("--entitlements"), "loose Mach-O files carry no entitlements");

    const entitlementCheck = calls.findIndex((c) => c.startsWith("codesign -d --entitlements"));
    const verify = calls.findIndex((c) => c.startsWith("codesign --verify --deep --strict"));
    assert.ok(calls.indexOf(signs[outerApp]) < entitlementCheck, "entitlements are compared after the outer signature");
    assert.ok(entitlementCheck < verify, "entitlements are compared before the deep verify");
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("finalizeMacosBundle: fails when signing changed the outer app's recorded entitlements", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-entitlement-drift-"));
  try {
    const fixture = createMacFinalizeFixture(tmp);
    const entitlements = join(fixture.workspaceDir, "outer-entitlements.plist");
    writeFileSync(entitlements, "<plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>\n");
    // The fixture's codesign -d prints no entitlements: the recorded one was lost.
    assert.throws(
      () =>
        finalizeMacosBundle({
          appPath: fixture.appPath,
          dmgPath: null,
          workspaceDir: fixture.workspaceDir,
          signingIdentity: "Developer ID Application: Indo Yoon (5DUM8WPB4C)",
          notaryProfile: "FerryxNotary",
          approveNotarization: true,
          entitlements,
          exec: fixture.exec,
        }),
      /Outer app entitlements changed/,
    );
    assert.equal(
      fixture.calls.some((c) => c.startsWith("xcrun notarytool submit")),
      false,
      "nothing is submitted for notarization after an entitlement drift",
    );
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

for (const appName of ["Ferryx.app", "Ferryx-2026.1003.1.app"]) {
test(`recreateDmgFromApp: packages ${appName} as Ferryx.app`, () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-dmg-recreate-"));
  try {
    const fixture = createMacFinalizeFixture(tmp);
    writeFileSync(join(fixture.appPath, "Contents", "MacOS", "ferryx"), "macho:main+signed");
    const appPath = join(tmp, appName);
    cpSync(fixture.appPath, appPath, { recursive: true });
    const stagingDir = join(fixture.workspaceDir, "dmg-staging");

    const { volumeName } = recreateDmgFromApp({
      appPath,
      dmgPath: fixture.dmgPath,
      stagingDir,
      exec: fixture.exec,
    });

    assert.equal(volumeName, "Ferryx");
    const snapshot = JSON.parse(readFileSync(fixture.dmgPath, "utf8"));
    assert.equal(snapshot["Ferryx.app/Contents/MacOS/ferryx"], "macho:main+signed");
    assert.deepEqual([...new Set(Object.keys(snapshot).map((entry) => entry.split(/[\\/]/)[0]))].sort(), ["Applications", "Ferryx.app"]);
    assert.equal(readFileSync(join(appPath, "Contents", "MacOS", "ferryx"), "utf8"), "macho:main+signed");
    assert.equal(existsSync(stagingDir), false, "staging directory must be cleaned up");
    assert.match(
      fixture.calls.find((c) => c.startsWith("hdiutil create")),
      /-volname Ferryx .*-srcfolder .*dmg-staging/,
    );
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});
}

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


test("floor gate: evaluateFloors accepts conforming artifacts and rejects each violated floor by name", () => {
  const winOk = ["KERNEL32.dll", "api-ms-win-crt-runtime-l1-1-0.dll", "WebView2Loader.dll"];
  const ok = evaluateFloors({
    kinds: ["cli-linux-amd64", "appimage", "deb", "macos-updater", "dmg", "cli-darwin-universal", "msix", "nsis", "cli-windows-amd64"],
    measurements: {
      "cli-linux-amd64": { glibc: "2.17" },
      appimage: { glibc: "2.35" },
      deb: { glibc: "2.34" },
      ...MAC_FLOORS,
      msix: { dependents: winOk },
      nsis: { dependents: winOk },
      "cli-windows-amd64": { dependents: ["KERNEL32.dll", "ucrtbase.dll"] },
    },
    macosMinimum: "10.15",
  });
  assert.equal(ok.length, 11);
  assert.deepEqual(ok.filter((c) => !c.pass), []);

  const bad = evaluateFloors({
    kinds: ["cli-linux-amd64", "appimage", "deb", "dmg", "cli-darwin-universal", "msix", "cli-windows-amd64"],
    measurements: {
      "cli-linux-amd64": { glibc: "2.18" },
      appimage: { glibc: "2.38" },
      deb: { glibc: "" },
      dmg: { minos: { x86_64: "11.0", arm64: "12.0" }, notarization: [AD_HOC("app"), AD_HOC("dmg")] },
      "cli-darwin-universal": { minos: null },
      msix: { dependents: ["KERNEL32.dll", "VCRUNTIME140.dll", "MSVCP140.dll"] },
      "cli-windows-amd64": {},
    },
    macosMinimum: "10.15",
  });
  const reasons = Object.fromEntries(bad.map((c) => [`${c.kind} ${c.check}`, c.pass ? "PASS" : c.reason]));
  assert.equal(reasons["cli-linux-amd64 glibc"], "requires GLIBC_2.18, above the GLIBC_2.17 floor");
  assert.equal(reasons["appimage glibc"], "requires GLIBC_2.38, above the GLIBC_2.35 floor");
  assert.equal(reasons["deb glibc"], "no GLIBC_ symbol version was measured");
  assert.equal(
    reasons["dmg macos-minos"],
    "x86_64 slice declares minos 11.0, above the configured minimum 10.15; arm64 slice declares minos 12.0, above 11.0 (the Apple Silicon floor)",
  );
  assert.match(reasons["dmg notarization"], /app: spctl does not report Notarized Developer ID \(app: rejected source=no usable signature\)/);
  assert.match(reasons["dmg notarization"], /dmg: stapler validate failed \(exit 65\)/);
  assert.equal(reasons["cli-darwin-universal macos-minos"], "no LC_BUILD_VERSION minos was measured");
  assert.equal(reasons["msix crt"], "depends on VCRUNTIME140.dll, MSVCP140.dll (Visual C++ runtime, absent on a clean Windows)");
  assert.equal(reasons["cli-windows-amd64 crt"], "no dumpbin /dependents output was captured");
  // A DMG whose app was notarized but whose image was never checked still fails.
  const [halfChecked] = evaluateFloors({
    kinds: ["dmg"],
    measurements: { dmg: { minos: { x86_64: "10.15", arm64: "11.0" }, notarization: [NOTARIZED("app")] } },
    macosMinimum: "10.15",
  }).filter((c) => c.check === "notarization");
  assert.equal(halfChecked.reason, "dmg: not checked");
  assert.throws(
    () => evaluateFloors({ kinds: ["dmg"], measurements: {}, macosMinimum: null }),
    /macOS minimum system version must be a dotted version/,
  );
});

test("parseFloorResult: reads the measurement line and fails closed without one", () => {
  const out = 'noise\n---FLOOR_RESULT---\n{"msix":{"dependents":["KERNEL32.dll"]}}\n---BUILD_RESULT---\n{"node":"1"}\n';
  assert.deepEqual(parseFloorResult(out), { msix: { dependents: ["KERNEL32.dll"] } });
  assert.throws(() => parseFloorResult("---BUILD_RESULT---\n{}\n"), /did not return floor measurements/);
});

test("measureMacosFloors: takes the highest minos per architecture over every Mach-O file and probes spctl and stapler", () => {
  const buildVersion = (minos) => `Load command 9\n      cmd LC_BUILD_VERSION\n  cmdsize 32\n platform 1\n    minos ${minos}\n      sdk 15.0\n`;
  const fakeExec = (x86AppMinos) => {
    const calls = [];
    const archs = {
      "/A.app/Contents/MacOS/ferryx": "x86_64 arm64",
      "/A.app/Contents/Helpers/ghostty-helper": "x86_64",
      "/out/ferryx-cli": "x86_64 arm64",
    };
    const otool = {
      "x86_64 /A.app/Contents/MacOS/ferryx": buildVersion(x86AppMinos),
      "arm64 /A.app/Contents/MacOS/ferryx": buildVersion("11.0"),
      "x86_64 /A.app/Contents/Helpers/ghostty-helper": "Load command 8\n      cmd LC_VERSION_MIN_MACOSX\n  cmdsize 16\n  version 10.13\n      sdk 10.14\n",
      "x86_64 /out/ferryx-cli": buildVersion("10.15"),
      "arm64 /out/ferryx-cli": buildVersion("11.0"),
    };
    const exec = (command, args) => {
      calls.push([command, ...args].join(" "));
      if (command === "find") return { status: 0, stdout: "/A.app/Contents/MacOS/ferryx\n/A.app/Contents/Info.plist\n/A.app/Contents/Helpers/ghostty-helper\n", stderr: "" };
      if (command === "file") return { status: 0, stdout: args[1].endsWith(".plist") ? "XML 1.0 document" : "Mach-O universal binary", stderr: "" };
      if (command === "lipo") return { status: 0, stdout: `${archs[args[1]]}\n`, stderr: "" };
      if (command === "otool") return { status: 0, stdout: otool[`${args[1]} ${args[3]}`], stderr: "" };
      if (command === "spctl") return { status: 3, stdout: "", stderr: `${args.at(-1)}: rejected\nsource=no usable signature` };
      if (command === "xcrun") return { status: 65, stdout: "", stderr: "does not have a ticket stapled to it" };
      throw new Error(`unexpected ${command}`);
    };
    return { exec, calls };
  };

  const { exec, calls } = fakeExec("10.15");
  const m = measureMacosFloors({ appPath: "/A.app", dmgPath: "/A.dmg", cliPath: "/out/ferryx-cli", exec });
  // The helper's LC_VERSION_MIN_MACOSX 10.13 is below the app's 10.15, so x86_64 stays 10.15.
  assert.deepEqual(m.dmg.minos, { x86_64: "10.15", arm64: "11.0" });
  assert.deepEqual(m["macos-updater"].minos, { x86_64: "10.15", arm64: "11.0" });
  assert.deepEqual(m["cli-darwin-universal"].minos, { x86_64: "10.15", arm64: "11.0" });
  assert.deepEqual(m.dmg.notarization.map((p) => [p.target, p.spctlStatus, p.staplerStatus]), [["app", 3, 65], ["dmg", 3, 65]]);
  assert.ok(calls.includes("lipo -archs /A.app/Contents/MacOS/ferryx"));
  assert.ok(calls.includes("otool -arch arm64 -l /A.app/Contents/MacOS/ferryx"));
  assert.ok(calls.includes("spctl -a -vvv -t install /A.dmg"));
  assert.ok(calls.includes("xcrun stapler validate /A.app"));
  assert.ok(!calls.some((c) => c.includes("Info.plist") && (c.startsWith("otool") || c.startsWith("lipo"))));
  // A universal build: arm64 at 11.0 meets the Apple Silicon floor; only the unsigned state fails.
  assert.deepEqual(
    evaluateFloors({ kinds: ["dmg", "cli-darwin-universal"], measurements: m, macosMinimum: "10.15" }).map((c) => `${c.kind} ${c.check} ${c.pass}`),
    ["dmg macos-minos true", "dmg notarization false", "cli-darwin-universal macos-minos true"],
  );

  // An Intel slice built for macOS 12 breaks the advertised 10.15 floor.
  const tooNew = measureMacosFloors({ appPath: "/A.app", exec: fakeExec("12.0").exec });
  const [minosCheck] = evaluateFloors({ kinds: ["macos-updater"], measurements: tooNew, macosMinimum: "10.15" });
  assert.equal(minosCheck.reason, "x86_64 slice declares minos 12.0, above the configured minimum 10.15");
});

function createFloorGateRun(tmp, platformSubdir) {
  const runDir = join(tmp, "run");
  for (const d of ["receipts", "bundles", join("artifacts", platformSubdir)]) mkdirSync(join(runDir, d), { recursive: true });
  writeFileSync(join(runDir, "bundles", "source.bundle"), "FIXTURE_SOURCE_BUNDLE");
  writeFileSync(join(runDir, "bundles", "ghostty.bundle"), "FIXTURE_GHOSTTY_BUNDLE");
  writeFileSync(join(runDir, "plan.json"), JSON.stringify({
    schemaVersion: 1,
    runId: "rel-run-floor-gate",
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
  }));
  const hostRoot = join(tmp, "host-root");
  mkdirSync(hostRoot);
  return { runDir, hostRoot };
}

const macRunner = (floors) => async ({ artifactsOutDir }) => {
  writeFileSync(join(artifactsOutDir, "Ferryx.app.tar.gz"), "MOCK_APP_CONTENT");
  writeFileSync(join(artifactsOutDir, "Ferryx.app.tar.gz.sig"), "untrusted comment: mock sig\nMOCK_SIGNATURE\n");
  writeFileSync(join(artifactsOutDir, "Ferryx_universal.dmg"), "MOCK_DMG_CONTENT");
  writeFileSync(join(artifactsOutDir, "ferryx-cli-darwin-universal"), "MOCK_CLI_CONTENT");
  return { exitCode: 0, toolchains: { node: "22.22.3", bun: "1.4.0", zig: "0.16.0", rust: "1.92.0" }, floors };
};

test("buildHost floor gate: an ad-hoc signed macOS build aborts with the named reason and writes no receipt", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-floor-gate-mac-"));
  try {
    const { runDir, hostRoot } = createFloorGateRun(tmp, "darwin");
    const config = {
      schemaVersion: 1, repository: join(tmp, "no-repo"), ghosttyRepository: join(tmp, "no-ghostty"), repo: "Indosaram/ferryx",
      hosts: { macbook: { platform: "darwin", ssh: null, root: hostRoot, minFreeBytes: 1024 } },
    };
    const adHoc = {
      "macos-updater": { minos: { x86_64: "10.15", arm64: "11.0" }, notarization: [AD_HOC("app")] },
      dmg: { minos: { x86_64: "10.15", arm64: "11.0" }, notarization: [AD_HOC("app"), AD_HOC("dmg")] },
      "cli-darwin-universal": { minos: { x86_64: "10.15", arm64: "11.0" } },
    };
    await assert.rejects(
      () => buildHost({ hostName: "macbook", config, runDir, runner: macRunner(adHoc), macosMinimum: "10.15", floorWaiver: null }),
      (err) => {
        assert.match(err.message, /^Release floor gate failed on host 'macbook': /);
        assert.match(err.message, /dmg notarization: app: spctl does not report Notarized Developer ID/);
        assert.match(err.message, /macos-updater notarization: app: spctl does not report Notarized Developer ID \([^)]*\); app: stapler validate failed \(exit 65\)/);
        assert.match(err.message, /FERRYX_RELEASE_FLOOR_WAIVER="<reason>"/);
        return true;
      },
    );
    assert.equal(existsSync(join(runDir, "receipts", "build-receipt-macbook.json")), false);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("buildHost floor gate: a local dev build may opt out explicitly; the waiver and failures are recorded", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-floor-gate-waiver-"));
  try {
    const { runDir, hostRoot } = createFloorGateRun(tmp, "darwin");
    const config = {
      schemaVersion: 1, repository: join(tmp, "no-repo"), ghosttyRepository: join(tmp, "no-ghostty"), repo: "Indosaram/ferryx",
      hosts: { macbook: { platform: "darwin", ssh: null, root: hostRoot, minFreeBytes: 1024 } },
    };
    const adHoc = { ...MAC_FLOORS, dmg: { minos: { x86_64: "10.15", arm64: "11.0" }, notarization: [AD_HOC("app"), AD_HOC("dmg")] } };
    const receipt = await buildHost({
      hostName: "macbook", config, runDir, runner: macRunner(adHoc), macosMinimum: "10.15", floorWaiver: "local QA build, not for publishing",
    });
    assert.equal(receipt.floors.waiver, "local QA build, not for publishing");
    assert.deepEqual(receipt.floors.checks.filter((c) => !c.pass).map((c) => `${c.kind}/${c.check}`), ["dmg/notarization"]);
    const saved = parseReceipt(readFileSync(join(runDir, "receipts", "build-receipt-macbook.json"), "utf8"));
    assert.equal(saved.floors.waiver, "local QA build, not for publishing");
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("buildHost floor gate: a Linux CLI needing GLIBC_2.38 aborts the omaki build", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-floor-gate-linux-"));
  try {
    const { runDir, hostRoot } = createFloorGateRun(tmp, "linux");
    // A non-ssh platform keeps the fixture runner local; the gate is host-independent.
    const config = {
      schemaVersion: 1, repository: join(tmp, "no-repo"), ghosttyRepository: join(tmp, "no-ghostty"), repo: "Indosaram/ferryx",
      hosts: { omaki: { platform: "fixture", ssh: null, root: hostRoot, minFreeBytes: 1024 } },
    };
    const linuxRunner = (floors) => async ({ artifactsOutDir }) => {
      writeFileSync(join(artifactsOutDir, "Ferryx_amd64.AppImage"), "MOCK_APPIMAGE");
      writeFileSync(join(artifactsOutDir, "Ferryx_amd64.AppImage.sig"), "untrusted comment: mock sig\nMOCK_SIGNATURE\n");
      writeFileSync(join(artifactsOutDir, "Ferryx_amd64.deb"), "MOCK_DEB");
      writeFileSync(join(artifactsOutDir, "ferryx-cli-linux-amd64"), "MOCK_CLI");
      return { exitCode: 0, toolchains: { node: "22.22.3", bun: "1.4.0", zig: "0.16.0", rust: "1.92.0" }, floors };
    };
    const linuxFloors = (cli) => ({ "cli-linux-amd64": { glibc: cli }, appimage: { glibc: "2.35" }, deb: { glibc: "2.35" } });
    await assert.rejects(
      () => buildHost({ hostName: "omaki", config, runDir, runner: linuxRunner(linuxFloors("2.38")), floorWaiver: null }),
      /Release floor gate failed on host 'omaki': cli-linux-amd64 glibc: requires GLIBC_2\.38, above the GLIBC_2\.17 floor\./,
    );
    assert.equal(existsSync(join(runDir, "receipts", "build-receipt-omaki.json")), false);

    // The same build with conforming measurements passes and records three glibc results.
    rmSync(join(runDir, "artifacts", "linux"), { recursive: true, force: true });
    mkdirSync(join(runDir, "artifacts", "linux"));
    rmSync(join(hostRoot, "rel-run-floor-gate"), { recursive: true, force: true });
    const receipt = await buildHost({ hostName: "omaki", config, runDir, runner: linuxRunner(linuxFloors("2.17")), floorWaiver: null });
    assert.deepEqual(receipt.floors.checks.map((c) => `${c.kind} ${c.measured}<=${c.limit} ${c.pass}`).sort(), [
      "appimage 2.35<=2.35 true",
      "cli-linux-amd64 2.17<=2.17 true",
      "deb 2.35<=2.35 true",
    ]);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});
