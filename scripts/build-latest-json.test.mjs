import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(REPO_ROOT, "scripts", "build-latest-json.mjs");
const REPO = "Indosaram/ferryx";
const TAG = "v2026.08.26.1";
const VERSION = "2026.08.26.1";

function seed(files) {
  const dir = mkdtempSync(join(tmpdir(), "latest-json-test-"));
  for (const [name, contents] of Object.entries(files)) {
    writeFileSync(join(dir, name), contents);
  }
  return dir;
}

function runScript(dir, extra = []) {
  const out = join(dir, "latest.json");
  const result = spawnSync(
    process.execPath,
    [
      SCRIPT,
      "--version",
      VERSION,
      "--dir",
      dir,
      "--out",
      out,
      "--repo",
      REPO,
      "--tag",
      TAG,
      ...extra,
    ],
    { encoding: "utf8" },
  );
  return { result, out };
}

test("a universal macOS bundle maps to both darwin targets with its real signature", () => {
  const dir = seed({
    "Ferryx_universal.app.tar.gz": "macos-bundle",
    "Ferryx_universal.app.tar.gz.sig": "  macos-signature-blob\n",
  });
  try {
    const { result, out } = runScript(dir);
    assert.equal(result.status, 0, `stderr: ${result.stderr}`);

    const manifest = JSON.parse(readFileSync(out, "utf8"));
    assert.equal(manifest.version, VERSION);
    assert.equal(Number.isNaN(Date.parse(manifest.pub_date)), false);
    assert.deepEqual(Object.keys(manifest.platforms).sort(), ["darwin-aarch64", "darwin-x86_64"]);

    for (const key of ["darwin-aarch64", "darwin-x86_64"]) {
      assert.equal(manifest.platforms[key].signature, "macos-signature-blob");
      assert.equal(
        manifest.platforms[key].url,
        `https://github.com/${REPO}/releases/download/${TAG}/Ferryx_universal.app.tar.gz`,
      );
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("windows and linux updater bundles land on their own targets", () => {
  const dir = seed({
    "Ferryx_x64-setup.nsis.zip": "windows-bundle",
    "Ferryx_x64-setup.nsis.zip.sig": "windows-signature",
    "Ferryx_amd64.AppImage.tar.gz": "linux-bundle",
    "Ferryx_amd64.AppImage.tar.gz.sig": "linux-signature",
  });
  try {
    const { result, out } = runScript(dir);
    assert.equal(result.status, 0, `stderr: ${result.stderr}`);

    const manifest = JSON.parse(readFileSync(out, "utf8"));
    assert.deepEqual(Object.keys(manifest.platforms).sort(), ["linux-x86_64", "windows-x86_64"]);
    assert.equal(manifest.platforms["windows-x86_64"].signature, "windows-signature");
    assert.equal(
      manifest.platforms["windows-x86_64"].url,
      `https://github.com/${REPO}/releases/download/${TAG}/Ferryx_x64-setup.nsis.zip`,
    );
    assert.equal(manifest.platforms["linux-x86_64"].signature, "linux-signature");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("Tauri v2 NSIS and AppImage artifacts land on their own targets", () => {
  const dir = seed({
    "Ferryx_2026.826.8_x64-setup.exe": "windows-nsis-bundle",
    "Ferryx_2026.826.8_x64-setup.exe.sig": "windows-nsis-signature",
    "Ferryx_2026.826.8_amd64.AppImage": "linux-appimage-bundle",
    "Ferryx_2026.826.8_amd64.AppImage.sig": "linux-appimage-signature",
  });
  try {
    const { result, out } = runScript(dir);
    assert.equal(result.status, 0, `stderr: ${result.stderr}`);

    const manifest = JSON.parse(readFileSync(out, "utf8"));
    assert.deepEqual(Object.keys(manifest.platforms).sort(), ["linux-x86_64", "windows-x86_64"]);
    assert.equal(manifest.platforms["windows-x86_64"].signature, "windows-nsis-signature");
    assert.equal(
      manifest.platforms["windows-x86_64"].url,
      `https://github.com/${REPO}/releases/download/${TAG}/Ferryx_2026.826.8_x64-setup.exe`,
    );
    assert.equal(manifest.platforms["linux-x86_64"].signature, "linux-appimage-signature");
    assert.equal(
      manifest.platforms["linux-x86_64"].url,
      `https://github.com/${REPO}/releases/download/${TAG}/Ferryx_2026.826.8_amd64.AppImage`,
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("an architecture-specific macOS bundle claims only its own target", () => {
  const dir = seed({
    "Ferryx_aarch64.app.tar.gz": "macos-arm-bundle",
    "Ferryx_aarch64.app.tar.gz.sig": "arm-signature",
  });
  try {
    const { result, out } = runScript(dir);
    assert.equal(result.status, 0, `stderr: ${result.stderr}`);

    const manifest = JSON.parse(readFileSync(out, "utf8"));
    assert.deepEqual(Object.keys(manifest.platforms), ["darwin-aarch64"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("an unsigned artifact is skipped with a warning instead of shipping unverifiable", () => {
  const dir = seed({
    "Ferryx_universal.app.tar.gz": "macos-bundle",
    "Ferryx_universal.app.tar.gz.sig": "macos-signature",
    "Ferryx_x64-setup.nsis.zip": "windows-bundle-without-signature",
  });
  try {
    const { result, out } = runScript(dir);
    assert.equal(result.status, 0, `stderr: ${result.stderr}`);
    assert.match(result.stderr, /Ferryx_x64-setup\.nsis\.zip/);

    const manifest = JSON.parse(readFileSync(out, "utf8"));
    assert.equal("windows-x86_64" in manifest.platforms, false);
    assert.deepEqual(Object.keys(manifest.platforms).sort(), ["darwin-aarch64", "darwin-x86_64"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("release notes are carried through when supplied", () => {
  const dir = seed({
    "Ferryx_universal.app.tar.gz": "macos-bundle",
    "Ferryx_universal.app.tar.gz.sig": "macos-signature",
  });
  try {
    const { result, out } = runScript(dir, ["--notes", "Adds in-app updates"]);
    assert.equal(result.status, 0, `stderr: ${result.stderr}`);
    assert.equal(JSON.parse(readFileSync(out, "utf8")).notes, "Adds in-app updates");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a directory with no signed updater artifact fails instead of publishing an empty manifest", () => {
  const dir = seed({ "SHA256SUMS.txt": "not-an-updater-artifact" });
  try {
    const { result } = runScript(dir);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /no signed updater artifact/i);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
