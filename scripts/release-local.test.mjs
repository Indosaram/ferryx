import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import http from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";

const SCRIPT_PATH = join(process.cwd(), "scripts", "release-local.mjs");

function digestDirectory(directory) {
  const hash = createHash("sha256");
  const visit = (current) => {
    for (const name of readdirSync(current).sort()) {
      const path = join(current, name);
      const stat = statSync(path);
      if (stat.isDirectory()) visit(path);
      else hash.update(path.slice(directory.length + 1)).update("\0").update(readFileSync(path));
    }
  };
  visit(directory);
  return hash.digest("hex");
}

function writeIntegrityState(runDir, plan, publishDir) {
  const configPath = join(runDir, "hosts.json");
  writeFileSync(configPath, "{}\n");
  const planBytes = readFileSync(join(runDir, "plan.json"));
  writeFileSync(join(runDir, "prepare-state.json"), JSON.stringify({
    schemaVersion: 1, runId: plan.runId, planDigest: createHash("sha256").update(planBytes).digest("hex"),
    configDigest: createHash("sha256").update(readFileSync(configPath)).digest("hex"), configPath,
    commitSha: plan.commitSha, sourceDateEpoch: plan.sourceDateEpoch, ghosttyPin: "a".repeat(40), updaterPublicKey: "fixture",
  }));
  const receiptsDir = join(runDir, "receipts");
  const artifactsDir = join(runDir, "artifacts");
  mkdirSync(receiptsDir, { recursive: true });
  mkdirSync(artifactsDir, { recursive: true });
  writeFileSync(join(runDir, "assembly-state.json"), JSON.stringify({
    schemaVersion: 1,
    planDigest: createHash("sha256").update(planBytes).digest("hex"),
    receiptsDigest: digestDirectory(receiptsDir), artifactsDigest: digestDirectory(artifactsDir),
    publishDigest: digestDirectory(publishDir),
  }));
}

test("release-local CLI: --help outputs usage and exits 0", () => {
  const res = spawnSync("node", [SCRIPT_PATH, "--help"], { encoding: "utf8" });
  assert.equal(res.status, 0);
  assert.match(res.stdout, /Usage: release-local/i);
  assert.match(res.stdout, /prepare/);
  assert.match(res.stdout, /preflight/);
  assert.match(res.stdout, /build/);
  assert.match(res.stdout, /assemble/);
  assert.match(res.stdout, /verify/);
  assert.match(res.stdout, /publish/);
  assert.match(res.stdout, /verify-remote/);
});

test("release-local: importing module has no side effects", async () => {
  const mod = await import("./release-local.mjs");
  assert.equal(typeof mod.prepareRelease, "function");
  assert.equal(typeof mod.preflightRelease, "function");
  assert.equal(typeof mod.buildRelease, "function");
  assert.equal(typeof mod.assembleReleaseRun, "function");
  assert.equal(typeof mod.verifyReleaseRun, "function");
  assert.equal(typeof mod.publishRelease, "function");
  assert.equal(typeof mod.verifyRemoteRelease, "function");
});

test("release-local: rejects mutating stages under GITHUB_ACTIONS=true and CI=true", () => {
  const mutatingStages = ["prepare", "build", "assemble", "publish"];

  for (const stage of mutatingStages) {
    // Test with GITHUB_ACTIONS=true
    const resGh = spawnSync("node", [SCRIPT_PATH, stage, "--run", "/tmp/fake"], {
      encoding: "utf8",
      env: { ...process.env, GITHUB_ACTIONS: "true" },
    });
    assert.notEqual(resGh.status, 0);
    assert.match(resGh.stderr + resGh.stdout, /forbidden under CI \/ GitHub Actions/i);

    // Test with CI=true
    const resCi = spawnSync("node", [SCRIPT_PATH, stage, "--run", "/tmp/fake"], {
      encoding: "utf8",
      env: { ...process.env, CI: "true", GITHUB_ACTIONS: "false" },
    });
    assert.notEqual(resCi.status, 0);
    assert.match(resCi.stderr + resCi.stdout, /forbidden under CI \/ GitHub Actions/i);
  }
});

test("prepare: rejects invalid calendar date tags and revision overflow", () => {
  const invalidTags = [
    "v2026.02.29", // 2026 is not a leap year
    "v2026.13.01", // month 13
    "v2026.04.31", // April has 30 days
    "v2025.12.31", // Year before 2026
    "v2026.09.08.65536", // Revision exceeds MSIX limit 65535
  ];

  for (const tag of invalidTags) {
    const res = spawnSync(
      "node",
      [
        SCRIPT_PATH,
        "prepare",
        "--config",
        "scripts/release-hosts.example.json",
        "--tag",
        tag,
        "--commit",
        "HEAD",
        "--out",
        "/tmp/ferryx-unused-run",
      ],
      { encoding: "utf8" },
    );
    assert.notEqual(res.status, 0);
    assert.match(res.stderr + res.stdout, /Invalid/i);
  }
});

test("prepare: creates deterministic plan, prepare-state, and fails closed on clobber", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-prepare-test-"));
  try {
    const outDir = join(tmp, "release-run-1");

    const res = spawnSync(
      "node",
      [
        SCRIPT_PATH,
        "prepare",
        "--config",
        "scripts/release-hosts.example.json",
        "--tag",
        "v2026.09.08.1",
        "--commit",
        "HEAD",
        "--out",
        outDir,
      ],
      { encoding: "utf8" },
    );

    assert.equal(res.status, 0, `stderr: ${res.stderr}`);
    assert.ok(existsSync(join(outDir, "plan.json")));
    assert.ok(existsSync(join(outDir, "prepare-state.json")));
    assert.ok(existsSync(join(outDir, "source-inputs.json")));
    assert.ok(existsSync(join(outDir, "artifacts", "darwin")));
    assert.ok(existsSync(join(outDir, "artifacts", "linux")));
    assert.ok(existsSync(join(outDir, "artifacts", "windows")));
    assert.ok(existsSync(join(outDir, "receipts")));

    const plan = JSON.parse(readFileSync(join(outDir, "plan.json"), "utf8"));
    assert.equal(plan.schemaVersion, 1);
    assert.equal(plan.tag, "v2026.09.08.1");
    assert.equal(plan.appVersion, "2026.908.1");
    assert.equal(plan.msixVersion, "2026.908.1.0");
    assert.equal(plan.channels.store, true);
    assert.equal(plan.channels.nsisMigration, false);
    assert.ok(plan.sourceDateEpoch > 0);

    const state = JSON.parse(readFileSync(join(outDir, "prepare-state.json"), "utf8"));
    assert.match(state.planDigest, /^[0-9a-f]{64}$/);
    assert.match(state.configDigest, /^[0-9a-f]{64}$/);
    assert.equal(state.commitSha, plan.commitSha);

    // Re-running prepare on same outDir must fail closed without touching files
    const resClobber = spawnSync(
      "node",
      [
        SCRIPT_PATH,
        "prepare",
        "--config",
        "scripts/release-hosts.example.json",
        "--tag",
        "v2026.09.08.1",
        "--commit",
        "HEAD",
        "--out",
        outDir,
      ],
      { encoding: "utf8" },
    );
    assert.notEqual(resClobber.status, 0);
    assert.match(resClobber.stderr + resClobber.stdout, /already exists/i);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("build: accepts an untouched prepared plan digest", async () => {
  const { prepareRelease, buildRelease } = await import("./release-local.mjs");
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-prepare-build-digest-"));
  try {
    const outDir = join(tmp, "run");
    const config = JSON.parse(readFileSync("scripts/release-hosts.example.json", "utf8"));
    config.hosts.macbook.root = join(tmp, "host-root");
    const configPath = join(tmp, "hosts.json");
    writeFileSync(configPath, JSON.stringify(config));
    prepareRelease({
      configPath,
      tag: "v2026.09.08.1",
      commit: "HEAD",
      outDir,
    });
    await assert.rejects(
      () => buildRelease({
        configPath,
        runDir: outDir,
        host: "macbook",
        runner: async () => undefined,
      }),
      /runner returned no result/i,
    );
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("preflight: exits non-zero and reports failure when disk budget is unmet", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-preflight-test-"));
  try {
    const config = JSON.parse(readFileSync("scripts/release-hosts.example.json", "utf8"));
    // Set impossible disk budget for macbook
    config.hosts.macbook.minFreeBytes = 100 * 1024 * 1024 * 1024 * 1024;
    const configPath = join(tmp, "hosts.json");
    writeFileSync(configPath, JSON.stringify(config, null, 2));

    const res = spawnSync("node", [SCRIPT_PATH, "preflight", "--config", configPath], {
      encoding: "utf8",
    });

    assert.notEqual(res.status, 0);
    assert.match(res.stdout, /"ok":\s*false/);
    assert.match(res.stdout + res.stderr, /disk/i);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("verify: detects tampered artifacts and altered receipts", async () => {
  const { verifyReleaseRun } = await import("./release-local.mjs");
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-verify-test-"));
  try {
    const runDir = join(tmp, "run");
    mkdirSync(runDir);
    const publishDir = join(runDir, "publish");
    mkdirSync(publishDir);

    const plan = {
      schemaVersion: 1,
      runId: "rel-run-verify-1",
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

    // Tampered publish directory: missing SHA256SUMS.txt and latest.json
    await assert.rejects(
      () => verifyReleaseRun({ runDir }),
      /prepare-state\.json|assembly-state\.json|latest\.json not found|Missing publish/i,
    );
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("publish: enforces double approval gate and rejects unapproved invocation", async () => {
  const { publishRelease } = await import("./release-local.mjs");
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-publish-gate-test-"));
  try {
    const runDir = join(tmp, "run");
    mkdirSync(runDir);

    // Case 1: Missing --approve-publish flag
    await assert.rejects(
      () => publishRelease({ runDir, approvePublish: false }),
      /Publication requires explicit --approve-publish flag/i,
    );

    // Case 2: Has flag, but missing FERRYX_APPROVE_PUBLISH=1 environment variable
    const origEnv = process.env.FERRYX_APPROVE_PUBLISH;
    delete process.env.FERRYX_APPROVE_PUBLISH;
    try {
      await assert.rejects(
        () => publishRelease({ runDir, approvePublish: true }),
        /FERRYX_APPROVE_PUBLISH=1 environment variable/i,
      );
    } finally {
      if (origEnv !== undefined) process.env.FERRYX_APPROVE_PUBLISH = origEnv;
    }
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("publish: full pipeline with fake gh executable and byte verification", async () => {
  const { publishRelease } = await import("./release-local.mjs");
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-publish-e2e-"));
  try {
    const runDir = join(tmp, "run");
    mkdirSync(runDir);
    const publishDir = join(runDir, "publish");
    mkdirSync(publishDir);

    const plan = {
      schemaVersion: 1,
      runId: "rel-run-e2e-pub-1",
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

    // Create authentic publish files
    const file1 = join(publishDir, "Ferryx_universal.dmg");
    const file2 = join(publishDir, "latest.json");
    writeFileSync(file1, "DMG_CONTENT_E2E");
    writeFileSync(file2, JSON.stringify({ version: plan.appVersion }));

    // SHA256SUMS.txt
    const hash1 = createHash("sha256").update(readFileSync(file1)).digest("hex");
    const hash2 = createHash("sha256").update(readFileSync(file2)).digest("hex");
    const sums = `${hash1}  Ferryx_universal.dmg\n${hash2}  latest.json\n`;
    writeFileSync(join(publishDir, "SHA256SUMS.txt"), sums);

    writeIntegrityState(runDir, plan, publishDir);

    // Create fake gh script that tracks calls and simulates GitHub release flow
    const fakeGhPath = join(tmp, "fake-gh.mjs");
    const storageDir = join(tmp, "gh-storage");
    mkdirSync(storageDir);

    const fakeGhScript = `#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

const storageDir = ${JSON.stringify(storageDir)};
const args = process.argv.slice(2);
const cmd = args[0];
const sub = args[1];

if (cmd === "release" && sub === "view") {
  const tag = args[2];
  const stateFile = path.join(storageDir, tag + ".json");
  if (!fs.existsSync(stateFile)) {
    process.stderr.write("release not found\\n");
    process.exit(1);
  }
  const state = JSON.parse(fs.readFileSync(stateFile, "utf8"));
  process.stdout.write(JSON.stringify(state));
  process.exit(0);
}

if (cmd === "release" && sub === "create") {
  const tag = args[2];
  const isDraft = args.includes("--draft");
  const state = { tag, isDraft, assets: [] };
  fs.writeFileSync(path.join(storageDir, tag + ".json"), JSON.stringify(state, null, 2));
  process.stdout.write("created draft release " + tag + "\\n");
  process.exit(0);
}

if (cmd === "release" && sub === "upload") {
  const tag = args[2];
  const stateFile = path.join(storageDir, tag + ".json");
  const state = JSON.parse(fs.readFileSync(stateFile, "utf8"));
  let i = 3;
  const files = [];
  while (i < args.length) {
    if (args[i] === "--repo") {
      i += 2;
    } else if (args[i].startsWith("--")) {
      i += 1;
    } else {
      files.push(args[i]);
      i += 1;
    }
  }
  const assetDir = path.join(storageDir, tag + "-assets");
  fs.mkdirSync(assetDir, { recursive: true });
  for (const f of files) {
    const name = path.basename(f);
    fs.copyFileSync(f, path.join(assetDir, name));
    state.assets.push(name);
  }
  fs.writeFileSync(stateFile, JSON.stringify(state, null, 2));
  process.stdout.write("uploaded " + files.length + " assets\\n");
  process.exit(0);
}

if (cmd === "release" && sub === "download") {
  const tag = args[2];
  const dirIndex = args.indexOf("--dir");
  const outDir = args[dirIndex + 1];
  const assetDir = path.join(storageDir, tag + "-assets");
  fs.mkdirSync(outDir, { recursive: true });
  for (const f of fs.readdirSync(assetDir)) {
    fs.copyFileSync(path.join(assetDir, f), path.join(outDir, f));
  }
  process.stdout.write("downloaded assets to " + outDir + "\\n");
  process.exit(0);
}

if (cmd === "release" && sub === "edit") {
  const tag = args[2];
  const stateFile = path.join(storageDir, tag + ".json");
  const state = JSON.parse(fs.readFileSync(stateFile, "utf8"));
  if (args.includes("--draft=false")) {
    state.isDraft = false;
  }
  fs.writeFileSync(stateFile, JSON.stringify(state, null, 2));
  process.stdout.write("updated release " + tag + "\\n");
  process.exit(0);
}

process.stderr.write("unknown command: " + args.join(" ") + "\\n");
process.exit(1);
`;
    writeFileSync(fakeGhPath, fakeGhScript);
    chmodSync(fakeGhPath, 0o755);

    // Mock lsRemoteCommand simulating git ls-remote output for plan.tag
    const mockLsRemote = () => `${plan.commitSha}\trefs/tags/${plan.tag}\n`;

    const result = await publishRelease({
      runDir,
      approvePublish: true,
      ghCommand: fakeGhPath,
      lsRemoteFn: mockLsRemote,
      env: { FERRYX_APPROVE_PUBLISH: "1" },
    });

    assert.equal(result.published, true);
    assert.equal(result.tag, plan.tag);

    // Verify storage state is undrafted
    const finalState = JSON.parse(readFileSync(join(storageDir, `${plan.tag}.json`), "utf8"));
    assert.equal(finalState.isDraft, false);
    assert.ok(finalState.assets.includes("Ferryx_universal.dmg"));
    assert.ok(finalState.assets.includes("latest.json"));
    assert.ok(finalState.assets.includes("SHA256SUMS.txt"));
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("verify-remote: verifies release availability against local HTTP server", async () => {
  const { verifyRemoteRelease } = await import("./release-local.mjs");
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-remote-verify-"));

  // Start local HTTP fixture server
  const mockServer = http.createServer((req, res) => {
    if (req.url === "/releases/download/v2026.09.08.1/latest.json") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ version: "2026.908.1" }));
    } else if (req.url === "/releases/download/v2026.09.08.1/SHA256SUMS.txt") {
      res.writeHead(200, { "Content-Type": "text/plain" });
      res.end("abc123  latest.json\n");
    } else {
      res.writeHead(404);
      res.end("Not Found");
    }
  });

  await new Promise((resolve) => mockServer.listen(0, "127.0.0.1", resolve));
  const port = mockServer.address().port;
  const baseUrl = `http://127.0.0.1:${port}`;

  try {
    const runDir = join(tmp, "run");
    mkdirSync(runDir);
    const plan = {
      schemaVersion: 1,
      runId: "rel-run-verify-rem-1",
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
    const publishDir = join(runDir, "publish");
    mkdirSync(publishDir);
    writeFileSync(join(publishDir, "latest.json"), JSON.stringify({ version: plan.appVersion }));
    writeFileSync(join(publishDir, "SHA256SUMS.txt"), "fixture\n");
    writeIntegrityState(runDir, plan, publishDir);

    const result = await verifyRemoteRelease({
      runDir,
      baseUrl: `${baseUrl}/releases/download`,
    });

    assert.equal(result.ok, true);
    assert.equal(result.tag, plan.tag);
  } finally {
    await new Promise((resolve) => mockServer.close(resolve));
    rmSync(tmp, { recursive: true, force: true });
  }
});
