import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const WORKFLOW = readFileSync(join(REPO_ROOT, ".github/workflows/release.yml"), "utf8");

test("the bundle step receives the updater signing secrets", () => {
  assert.match(WORKFLOW, /TAURI_SIGNING_PRIVATE_KEY: \$\{\{ secrets\.TAURI_SIGNING_PRIVATE_KEY \}\}/);
  assert.match(
    WORKFLOW,
    /TAURI_SIGNING_PRIVATE_KEY_PASSWORD: \$\{\{ secrets\.TAURI_SIGNING_PRIVATE_KEY_PASSWORD \}\}/,
  );
});

test("the macOS unsigned fallback never unsets the updater signing key", () => {
  const fallback = WORKFLOW.slice(
    WORKFLOW.indexOf("Skipping macOS codesigning"),
    WORKFLOW.indexOf("bunx @tauri-apps/cli build"),
  );
  assert.notEqual(fallback.length, 0);
  assert.equal(fallback.includes("unset TAURI_SIGNING_PRIVATE_KEY"), false);
});

test("both bundling jobs and the manifest derive a semver from the release tag", () => {
  const occurrences = WORKFLOW.match(/node scripts\/sync-version\.mjs --tag/g) ?? [];
  assert.equal(
    occurrences.length,
    3,
    "build-desktop, build-msix, and latest.json must derive the same semver",
  );
});

test("signatures and updater bundles are collected as release artifacts", () => {
  assert.match(WORKFLOW, /-name "\*\.sig"/);
  assert.match(WORKFLOW, /-name "\*\.nsis\.zip"/);
  assert.match(WORKFLOW, /-name "\*\.AppImage\.tar\.gz"/);
});

test("the publish job generates latest.json into the uploaded directory", () => {
  assert.match(WORKFLOW, /node scripts\/build-latest-json\.mjs/);
  assert.match(WORKFLOW, /MANIFEST_VERSION="\$\(node scripts\/sync-version\.mjs --tag/);
  assert.match(WORKFLOW, /--version "\$\{MANIFEST_VERSION\}"/);
  assert.match(WORKFLOW, /--out release-dist\/latest\.json/);
  assert.match(WORKFLOW, /files: release-dist\/\*/);
});

test("release workflow never runs from a tag push", () => {
  assert.doesNotMatch(WORKFLOW, /^\s*push:\s*$/m);
  assert.match(WORKFLOW, /^\s*workflow_dispatch:\s*$/m);
});
