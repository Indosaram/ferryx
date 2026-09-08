import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const POLICY_BIN = join(REPO_ROOT, "scripts/release-workflow-policy.mjs");
const BUILD_TEST_PATH = join(REPO_ROOT, ".github/workflows/build-test.yml");
const PAGES_PATH = join(REPO_ROOT, ".github/workflows/deploy-pages.yml");
const RELEASE_WORKFLOW_PATH = join(REPO_ROOT, ".github/workflows/release.yml");

// Helper to run the policy validator via Bun
function runPolicy(args = [], options = {}) {
  return spawnSync("bun", [POLICY_BIN, ...args], {
    encoding: "utf8",
    ...options,
  });
}

// Frozen fixture of the retired hosted producer (.github/workflows/release.yml)
const RETIRED_PRODUCER_FIXTURE = `name: Release Ferryx

'on':
  push:
    tags:
      - 'v[0-9][0-9][0-9][0-9].[0-9][0-9].[0-9][0-9]'
      - 'v[0-9][0-9][0-9][0-9].[0-9][0-9].[0-9][0-9].[0-9]*'
  workflow_dispatch:

permissions:
  contents: write

jobs:
  build-desktop:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - name: Import macOS signing credentials
        env:
          APPLE_CERTIFICATE: \${{ secrets.APPLE_CERTIFICATE }}
          KEYCHAIN_PASSWORD: \${{ secrets.KEYCHAIN_PASSWORD }}
        run: |
          security create-keychain -p "$KEYCHAIN_PASSWORD" build.keychain
          security import cert.p12 -k build.keychain
      - name: Build Tauri Bundle
        env:
          TAURI_SIGNING_PRIVATE_KEY: \${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
        run: bunx @tauri-apps/cli build --target universal-apple-darwin
      - name: Verify updater layout
        run: node scripts/assert-updater-archive-layout.mjs test.tar.gz
  build-msix:
    runs-on: windows-latest
    steps:
      - run: .\\scripts\\build-msix.ps1 -Version "2026.908.1.0"
  publish-release:
    needs: [build-desktop, build-msix]
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - run: node scripts/build-latest-json.mjs --version "2026.908.1"
      - uses: softprops/action-gh-release@v2
        with:
          tag_name: \${{ github.ref_name }}
`;

test("actual current producer fixture is rejected by parsed-policy validator", (t) => {
  const tempDir = mkdtempSync(join(tmpdir(), "policy-test-"));
  t.after(() => rmSync(tempDir, { recursive: true, force: true }));

  const fixturePath = join(tempDir, "release.yml");
  writeFileSync(fixturePath, RETIRED_PRODUCER_FIXTURE, "utf8");

  const res = runPolicy([fixturePath]);
  assert.equal(res.status, 1, `Expected exit 1, got ${res.status}. Output: ${res.stderr || res.stdout}`);
  const combined = (res.stderr + res.stdout);
  assert.match(combined, /release\.yml/i);
  assert.match(combined, /contents:\s*write/i);
  assert.match(combined, /TAURI_SIGNING_PRIVATE_KEY/);
  assert.match(combined, /APPLE_CERTIFICATE/);
  assert.match(combined, /tauri.*build/i);
  assert.match(combined, /build-msix\.ps1/);
  assert.match(combined, /action-gh-release/);
  assert.match(combined, /build-latest-json\.mjs/);
});

test("manual release producer fixture (workflow_dispatch) is rejected", (t) => {
  const tempDir = mkdtempSync(join(tmpdir(), "policy-test-"));
  t.after(() => rmSync(tempDir, { recursive: true, force: true }));

  const fixture = `name: Manual Producer Backdoor
on:
  workflow_dispatch:

permissions:
  contents: write

jobs:
  manual-build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: bunx @tauri-apps/cli build
`;
  const fixturePath = join(tempDir, "manual-backdoor.yml");
  writeFileSync(fixturePath, fixture, "utf8");

  const res = runPolicy([fixturePath]);
  assert.equal(res.status, 1, `Expected exit 1, got ${res.status}`);
  const combined = res.stderr + res.stdout;
  assert.match(combined, /contents:\s*write/i);
  assert.match(combined, /tauri.*build/i);
});

test("tag release producer fixture (push.tags) is rejected", (t) => {
  const tempDir = mkdtempSync(join(tmpdir(), "policy-test-"));
  t.after(() => rmSync(tempDir, { recursive: true, force: true }));

  const fixture = `name: Tag Release Producer
on:
  push:
    tags:
      - 'v*'

jobs:
  tag-publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: softprops/action-gh-release@v2
        with:
          tag_name: \${{ github.ref_name }}
`;
  const fixturePath = join(tempDir, "tag-producer.yml");
  writeFileSync(fixturePath, fixture, "utf8");

  const res = runPolicy([fixturePath]);
  assert.equal(res.status, 1, `Expected exit 1, got ${res.status}`);
  const combined = res.stderr + res.stdout;
  assert.match(combined, /push\.tags/i);
  assert.match(combined, /action-gh-release/i);
});

test("callable release producer fixture (workflow_call) is rejected", (t) => {
  const tempDir = mkdtempSync(join(tmpdir(), "policy-test-"));
  t.after(() => rmSync(tempDir, { recursive: true, force: true }));

  const fixture = `name: Callable Release Job
on:
  workflow_call:

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - run: gh release create "v2026.09.08" dist/*
`;
  const fixturePath = join(tempDir, "callable-producer.yml");
  writeFileSync(fixturePath, fixture, "utf8");

  const res = runPolicy([fixturePath]);
  assert.equal(res.status, 1, `Expected exit 1, got ${res.status}`);
  const combined = res.stderr + res.stdout;
  assert.match(combined, /gh release/i);
});

test("reusable release producer fixture (workflow_call with release signing secrets) is rejected", (t) => {
  const tempDir = mkdtempSync(join(tmpdir(), "policy-test-"));
  t.after(() => rmSync(tempDir, { recursive: true, force: true }));

  const fixture = `name: Reusable Signer
on:
  workflow_call:
    secrets:
      TAURI_SIGNING_PRIVATE_KEY:
        required: true

jobs:
  sign:
    runs-on: macos-latest
    steps:
      - run: echo "Signing with private key..."
`;
  const fixturePath = join(tempDir, "reusable-signer.yml");
  writeFileSync(fixturePath, fixture, "utf8");

  const res = runPolicy([fixturePath]);
  assert.equal(res.status, 1, `Expected exit 1, got ${res.status}`);
  const combined = res.stderr + res.stdout;
  assert.match(combined, /TAURI_SIGNING_PRIVATE_KEY/);
});

test("rejects 'bun tauri build' release entry point fixture", (t) => {
  const tempDir = mkdtempSync(join(tmpdir(), "policy-test-"));
  t.after(() => rmSync(tempDir, { recursive: true, force: true }));

  const fixture = `name: Bun Tauri Build Backdoor
on:
  pull_request:
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: bun tauri build
`;
  const fixturePath = join(tempDir, "bun-tauri-build.yml");
  writeFileSync(fixturePath, fixture, "utf8");

  const res = runPolicy([fixturePath]);
  assert.equal(res.status, 1, `Expected exit 1, got ${res.status}`);
  const combined = res.stderr + res.stdout;
  assert.match(combined, /tauri.*build/i);
});

test("rejects root 'bun run build' delegation fixture while permitting scoped ui/site builds", (t) => {
  const tempDir = mkdtempSync(join(tmpdir(), "policy-test-"));
  t.after(() => rmSync(tempDir, { recursive: true, force: true }));

  const fixture = `name: Root Build Delegation
on:
  pull_request:
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: bun run build
`;
  const fixturePath = join(tempDir, "root-build.yml");
  writeFileSync(fixturePath, fixture, "utf8");

  const res = runPolicy([fixturePath]);
  assert.equal(res.status, 1, `Expected exit 1, got ${res.status}`);
  const combined = res.stderr + res.stdout;
  assert.match(combined, /root.*build/i);
});

test("rejects 'tauri-apps/tauri-action' action in step uses", (t) => {
  const tempDir = mkdtempSync(join(tmpdir(), "policy-test-"));
  t.after(() => rmSync(tempDir, { recursive: true, force: true }));

  const fixture = `name: Tauri Action Check
on:
  pull_request:
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: tauri-apps/tauri-action@v0
`;
  const fixturePath = join(tempDir, "tauri-action.yml");
  writeFileSync(fixturePath, fixture, "utf8");

  const res = runPolicy([fixturePath]);
  assert.equal(res.status, 1, `Expected exit 1, got ${res.status}`);
  const combined = res.stderr + res.stdout;
  assert.match(combined, /tauri-action/i);
});

test("rejects 'scripts/release-local.mjs' coordinator invocation in workflow", (t) => {
  const tempDir = mkdtempSync(join(tmpdir(), "policy-test-"));
  t.after(() => rmSync(tempDir, { recursive: true, force: true }));

  const fixture = `name: Local Release Coordinator in CI
on:
  pull_request:
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: node scripts/release-local.mjs publish --plan release-plan.json
`;
  const fixturePath = join(tempDir, "release-local.yml");
  writeFileSync(fixturePath, fixture, "utf8");

  const res = runPolicy([fixturePath]);
  assert.equal(res.status, 1, `Expected exit 1, got ${res.status}`);
  const combined = res.stderr + res.stdout;
  assert.match(combined, /release-local\.mjs/i);
});

test("rejects reusable job uses of release workflows and secrets: inherit", (t) => {
  const tempDir = mkdtempSync(join(tmpdir(), "policy-test-"));
  t.after(() => rmSync(tempDir, { recursive: true, force: true }));

  const fixture = `name: Reusable Release Job
on:
  pull_request:
jobs:
  reusable-release:
    uses: ./.github/workflows/release-reusable.yml
    secrets: inherit
`;
  const fixturePath = join(tempDir, "reusable-job.yml");
  writeFileSync(fixturePath, fixture, "utf8");

  const res = runPolicy([fixturePath]);
  assert.equal(res.status, 1, `Expected exit 1, got ${res.status}`);
  const combined = res.stderr + res.stdout;
  assert.match(combined, /reusable-release|secrets:\s*inherit/i);
});

test("PR check workflow (.github/workflows/build-test.yml) complies with release policy", () => {
  const res = runPolicy([BUILD_TEST_PATH]);
  assert.equal(
    res.status,
    0,
    `build-test.yml failed policy check: ${res.stderr || res.stdout}`,
  );
});

test("Pages deployment workflow (.github/workflows/deploy-pages.yml) complies with release policy", () => {
  const res = runPolicy([PAGES_PATH]);
  assert.equal(
    res.status,
    0,
    `deploy-pages.yml failed policy check: ${res.stderr || res.stdout}`,
  );
});

test("PR check workflow wires the release workflow policy check", () => {
  const content = readFileSync(BUILD_TEST_PATH, "utf8");
  assert.match(
    content,
    /bun\s+scripts\/release-workflow-policy\.mjs/,
    "build-test.yml must execute release-workflow-policy.mjs",
  );
});

test("Windows CI links the native binary before a release tag", () => {
  const content = readFileSync(BUILD_TEST_PATH, "utf8");
  assert.match(content, /name: Cargo Link \(Windows\)/);
  assert.match(
    content,
    /if: matrix\.os_name == 'windows'[\s\S]*cargo build --manifest-path src-tauri\/Cargo\.toml --target \$\{\{ matrix\.target \}\}/,
  );
});

test("hosted release producer .github/workflows/release.yml is absent from disk", () => {
  assert.equal(
    existsSync(RELEASE_WORKFLOW_PATH),
    false,
    ".github/workflows/release.yml must be removed from repository to prevent backdoor release runs",
  );
});

test("live repository .github/workflows directory complies fully with policy", () => {
  const workflowsDir = join(REPO_ROOT, ".github/workflows");
  const res = runPolicy([workflowsDir]);
  assert.equal(
    res.status,
    0,
    `.github/workflows directory failed policy check: ${res.stderr || res.stdout}`,
  );
});
