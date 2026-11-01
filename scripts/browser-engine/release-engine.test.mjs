import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  assetUrl,
  buildManifest,
  codesignArgs,
  engineVersion,
  keygen,
  macSigningPlan,
  parseArgs,
  platformKey,
  publish,
  REPO_ROOT,
  sha256File,
  verifyManifestSignature,
  verifyReleaseDir,
  writeManifest,
} from "./release-engine.mjs";

function scratch() {
  return mkdtempSync(join(tmpdir(), "ferryx-engine-test-"));
}

function releaseDir(version) {
  const dir = scratch();
  writeFileSync(join(dir, "engine-macos-aarch64.zip"), Buffer.from("not really a zip, but bytes"));
  const zip = join(dir, "engine-macos-aarch64.zip");
  writeFileSync(
    join(dir, "engine-macos-aarch64.json"),
    JSON.stringify({ version, platform: "macos-aarch64", file: "engine-macos-aarch64.zip", sha256: sha256File(zip), size: statSync(zip).size, codeSigned: false }),
  );
  return dir;
}

test("the engine version is read from engine_install.rs", () => {
  assert.match(engineVersion(), /^\d+\.\d+\.\d+-ferryx\.\d+$/);
  assert.equal(engineVersion('pub const ENGINE_VERSION: &str = "1.2.3-ferryx.4";'), "1.2.3-ferryx.4");
  assert.throws(() => engineVersion("nothing here"), /ENGINE_VERSION/);
});

test("platform keys match the app's", () => {
  assert.equal(platformKey("darwin", "arm64"), "macos-aarch64");
  assert.equal(platformKey("win32", "x64"), "windows-x86_64");
  assert.equal(platformKey("linux", "x64"), null);
});

test("keygen refuses a path inside the repository", () => {
  assert.throws(() => keygen({ out: join(REPO_ROOT, "keys") }), /inside the repository/);
});

test("a signed manifest verifies, and a tampered one or another key does not", () => {
  const keys = scratch();
  const other = scratch();
  const version = engineVersion();
  const dir = releaseDir(version);
  try {
    const { keyPath, publicKey } = keygen({ out: keys });
    if (process.platform !== "win32") assert.equal(statSync(keyPath).mode & 0o777, 0o600);
    const { publicKey: otherKey } = keygen({ out: other });
    const { manifest, unsigned } = writeManifest({ dir, keyPath, version });
    assert.deepEqual(unsigned, ["macos-aarch64"]);
    assert.equal(manifest.platforms["macos-aarch64"].url, assetUrl(version, "engine-macos-aarch64.zip"));
    assert.equal(verifyReleaseDir({ dir, publicKey }).version, version);

    const bytes = readFileSync(join(dir, "manifest.json"));
    const signature = readFileSync(join(dir, "manifest.json.sig"), "utf8");
    assert.equal(verifyManifestSignature(bytes, signature, publicKey), true);
    assert.equal(verifyManifestSignature(bytes, signature, otherKey), false);
    const tampered = Buffer.from(bytes.toString("utf8").replace(/"size": \d+/, '"size": 1'));
    assert.equal(verifyManifestSignature(tampered, signature, publicKey), false);
    assert.throws(() => verifyReleaseDir({ dir, publicKey: otherKey }), /does not verify/);

    writeFileSync(join(dir, "engine-macos-aarch64.zip"), "changed after signing!!!!!!");
    assert.throws(() => verifyReleaseDir({ dir, publicKey }), /sha256 differs/);
  } finally {
    for (const path of [keys, other, dir]) rmSync(path, { recursive: true, force: true });
  }
});

test("the manifest refuses a record built as another version", () => {
  assert.throws(
    () => buildManifest({ version: "2.0.0-ferryx.1", records: [{ version: "1.0.0-ferryx.1", platform: "macos-aarch64", file: "a.zip", sha256: "x", size: 1 }] }),
    /built as 1\.0\.0/,
  );
  assert.throws(() => buildManifest({ version: "2.0.0-ferryx.1", records: [] }), /no engine/);
});

test("macOS signing goes inside out with the right entitlements", () => {
  const listing = {
    "/x/ferryx_engine.app/Contents/Frameworks/Chromium Embedded Framework.framework/Libraries": ["libEGL.dylib", "vk.json", "libcef_sandbox.dylib"],
    "/x/ferryx_engine.app/Contents/Frameworks": ["Chromium Embedded Framework.framework", "ferryx_engine Helper.app", "ferryx_engine Helper (Renderer).app"],
  };
  const plan = macSigningPlan("/x/ferryx_engine.app", (dir) => listing[dir] ?? []);
  const names = plan.map((step) => step.path.split("/").pop());
  assert.deepEqual(names, [
    "libEGL.dylib",
    "libcef_sandbox.dylib",
    "Chromium Embedded Framework.framework",
    "ferryx_engine Helper (Renderer).app",
    "ferryx_engine Helper.app",
    "ferryx_engine.app",
  ]);
  assert.match(plan.at(-1).entitlements, /engine\.plist$/);
  assert.match(plan[3].entitlements, /helper\.plist$/);
  const args = codesignArgs("Developer ID Application: Test (ABC)", plan.at(-1));
  assert.deepEqual(args.slice(0, 6), ["--force", "--timestamp", "--options", "runtime", "--sign", "Developer ID Application: Test (ABC)"]);
  for (const step of plan) {
    if (step.entitlements) assert.ok(statSync(step.entitlements).isFile(), step.entitlements);
  }
});

test("publish without --publish only prints the plan, and never runs under CI", () => {
  const keys = scratch();
  const version = engineVersion();
  const dir = releaseDir(version);
  try {
    const { keyPath, publicKey } = keygen({ out: keys });
    writeManifest({ dir, keyPath, version });
    const lines = [];
    const result = publish({ dir, publishFlag: false, publicKey, env: {}, log: (line) => lines.push(line) });
    assert.equal(result.published, false);
    assert.deepEqual(result.plan.map((command) => command[2]), ["create", "upload", "download", "edit"]);
    assert.ok(result.plan[0].includes("--draft"));
    assert.ok(result.plan[0].includes(`browser-engine-${version}`));
    assert.ok(lines.some((line) => line.includes("gh release upload")));
    assert.throws(() => publish({ dir, publishFlag: true, publicKey, env: { CI: "true" } }), /local-only/);
  } finally {
    for (const path of [keys, dir]) rmSync(path, { recursive: true, force: true });
  }
});

test("arguments parse flags and values", () => {
  assert.deepEqual(parseArgs(["publish", "--dir", "/d", "--publish"]), { command: "publish", options: { dir: "/d", publish: true } });
  assert.throws(() => parseArgs(["manifest", "--dir"]), /needs a value/);
  assert.throws(() => parseArgs(["manifest", "loose"]), /unexpected/);
});
