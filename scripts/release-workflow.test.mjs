import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const WORKFLOW = readFileSync(join(REPO_ROOT, ".github/workflows/release.yml"), "utf8");
const BUILD_TEST_WORKFLOW = readFileSync(join(REPO_ROOT, ".github/workflows/build-test.yml"), "utf8");

test("the bundle step receives the updater signing secrets", () => {
  assert.match(WORKFLOW, /TAURI_SIGNING_PRIVATE_KEY: \$\{\{ secrets\.TAURI_SIGNING_PRIVATE_KEY \}\}/);
  assert.match(
    WORKFLOW,
    /TAURI_SIGNING_PRIVATE_KEY_PASSWORD: \$\{\{ secrets\.TAURI_SIGNING_PRIVATE_KEY_PASSWORD \}\}/,
  );
});

test("release builds use Zig setup v2 for Zig 0.16 archives", () => {
  const setupActionReferences = WORKFLOW.match(/uses: mlugg\/setup-zig@v\d+/g) ?? [];
  assert.deepEqual(setupActionReferences, [
    "uses: mlugg/setup-zig@v2",
    "uses: mlugg/setup-zig@v2",
  ]);
  assert.doesNotMatch(WORKFLOW, /uses: mlugg\/setup-zig@v1/);
});

test("the macOS build imports its Developer ID identity and configures notarization", () => {
  assert.match(WORKFLOW, /name: Import macOS signing and notarization credentials/);
  assert.match(WORKFLOW, /APPLE_CERTIFICATE: \$\{\{ secrets\.APPLE_CERTIFICATE \}\}/);
  assert.match(WORKFLOW, /APPLE_CERTIFICATE_PASSWORD: \$\{\{ secrets\.APPLE_CERTIFICATE_PASSWORD \}\}/);
  assert.match(WORKFLOW, /KEYCHAIN_PASSWORD: \$\{\{ secrets\.KEYCHAIN_PASSWORD \}\}/);
  assert.match(WORKFLOW, /security import "\$CERTIFICATE_PATH"/);
  assert.match(WORKFLOW, /security set-key-partition-list/);
  assert.match(WORKFLOW, /APPLE_API_KEY_CONTENT: \$\{\{ secrets\.APPLE_API_KEY_CONTENT \}\}/);
  assert.match(WORKFLOW, /APPLE_API_KEY_PATH=/);
  assert.match(WORKFLOW, /APPLE_API_ISSUER: \$\{\{ secrets\.APPLE_API_ISSUER \}\}/);
});

test("the macOS bundle inherits its imported identity and App Store Connect key path", () => {
  assert.match(WORKFLOW, /APPLE_SIGNING_IDENTITY: \$\{\{ secrets\.APPLE_SIGNING_IDENTITY \}\}/);
  assert.match(WORKFLOW, /APPLE_API_KEY_PATH: \$\{\{ env\.APPLE_API_KEY_PATH \}\}/);
  assert.doesNotMatch(WORKFLOW, /Skipping macOS codesigning/);
  assert.doesNotMatch(WORKFLOW, /unset TAURI_SIGNING_PRIVATE_KEY/);
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

test("macOS updater archives are scrubbed and validated before publishing", () => {
  assert.match(WORKFLOW, /Remove AppleDouble metadata before macOS bundling/);
  assert.match(WORKFLOW, /find ui src-tauri -type f -name '._\*' -delete/);
  assert.match(WORKFLOW, /Verify macOS updater archive layout/);
  assert.match(WORKFLOW, /find src-tauri\/target -type f -name '\*\.app\.tar\.gz' -print0/);
  assert.match(WORKFLOW, /node scripts\/assert-updater-archive-layout\.mjs/);
});

test("the publish job generates latest.json into the uploaded directory", () => {
  assert.match(WORKFLOW, /node scripts\/build-latest-json\.mjs/);
  assert.match(WORKFLOW, /MANIFEST_VERSION="\$\(node scripts\/sync-version\.mjs --tag/);
  assert.match(WORKFLOW, /--version "\$\{MANIFEST_VERSION\}"/);
  assert.match(WORKFLOW, /--out release-dist\/latest\.json/);
  assert.match(WORKFLOW, /files: release-dist\/\*/);
});

test("release workflow triggers on date-versioned tag pushes and retains manual dispatch", () => {
  assert.match(
    WORKFLOW,
    /push:\s*\n\s*tags:\s*\n\s*-\s*['"]v\[0-9\]\[0-9\]\[0-9\]\[0-9\]\.\[0-9\]\[0-9\]\.\[0-9\]\[0-9\]['"]\s*\n\s*-\s*['"]v\[0-9\]\[0-9\]\[0-9\]\[0-9\]\.\[0-9\]\[0-9\]\.\[0-9\]\[0-9\]\.\[0-9\]\*['"]/,
  );
  assert.match(WORKFLOW, /^\s*workflow_dispatch:\s*$/m);
  assert.doesNotMatch(WORKFLOW, /^\s*branches:\s*$/m);
});

test("Windows CI links the native binary before a release tag", () => {
  assert.match(BUILD_TEST_WORKFLOW, /name: Cargo Link \(Windows\)/);
  assert.match(
    BUILD_TEST_WORKFLOW,
    /if: matrix\.os_name == 'windows'[\s\S]*cargo build --manifest-path src-tauri\/Cargo\.toml --target \$\{\{ matrix\.target \}\}/,
  );
});

test("the date-based updater release builds only the NSIS bundle on Windows to avoid WiX's MSI version ceiling", () => {
  assert.match(
    WORKFLOW,
    /platform:\s*['"]windows-latest['"][\s\S]*?tauri_args:\s*['"][^'"]*--bundles\s+nsis[^'"]*['"]/,
  );
  assert.doesNotMatch(WORKFLOW, /\| \*\*Windows\*\* \| `\.msi`/);
});
