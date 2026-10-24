import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { HOMEBREW_PACKAGE_NAMES, parseChecksums, renderHomebrewPackages } from "./release-packaging.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(REPO_ROOT, "scripts", "release-packaging.mjs");
const FIXTURES = join(REPO_ROOT, "scripts", "fixtures", "homebrew");
const SUMS = readFileSync(join(FIXTURES, "SHA256SUMS"), "utf8");
const TAG = "v2026.10.24.2";

test("renders cask and formula whose version and hashes match SHA256SUMS", () => {
  const sums = parseChecksums(SUMS);
  const rendered = renderHomebrewPackages({ checksums: SUMS, tag: TAG });
  assert.deepEqual(Object.keys(rendered), HOMEBREW_PACKAGE_NAMES);

  const cask = rendered["ferryx.rb"];
  assert.match(cask, /^cask "ferryx" do$/m);
  assert.match(cask, /^ {2}version "2026\.10\.24\.2"$/m);
  assert.ok(cask.includes(`sha256 "${sums.get("Ferryx_universal.dmg")}"`));
  assert.ok(cask.includes(`url "https://github.com/Indosaram/ferryx/releases/download/${TAG}/Ferryx_universal.dmg"`));

  const formula = rendered["ferryx-cli.rb"];
  assert.match(formula, /^class FerryxCli < Formula$/m);
  assert.match(formula, /^ {2}version "2026\.10\.24\.2"$/m);
  assert.ok(formula.includes(`sha256 "${sums.get("ferryx-cli-linux-amd64")}"`));
  assert.ok(formula.includes(`url "https://github.com/Indosaram/ferryx/releases/download/${TAG}/ferryx-cli-linux-amd64"`));
});

test("CLI output diffs cleanly against the golden fixtures", () => {
  const out = mkdtempSync(join(tmpdir(), "ferryx-homebrew-"));
  try {
    const res = spawnSync(process.execPath, [SCRIPT, "--sums", join(FIXTURES, "SHA256SUMS"), "--tag", TAG, "--out", out], {
      encoding: "utf8",
    });
    assert.equal(res.status, 0, res.stdout + res.stderr);
    for (const name of HOMEBREW_PACKAGE_NAMES) {
      assert.equal(readFileSync(join(out, name), "utf8"), readFileSync(join(FIXTURES, name), "utf8"), name);
    }
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("a missing asset in SHA256SUMS exits non-zero and writes nothing", () => {
  const out = mkdtempSync(join(tmpdir(), "ferryx-homebrew-missing-"));
  try {
    const sumsPath = join(out, "SHA256SUMS");
    writeFileSync(sumsPath, SUMS.split("\n").filter((line) => !line.endsWith("  ferryx-cli-linux-amd64")).join("\n"));
    const res = spawnSync(process.execPath, [SCRIPT, "--sums", sumsPath, "--tag", TAG, "--out", out], { encoding: "utf8" });
    assert.notEqual(res.status, 0);
    assert.match(res.stderr, /ferryx-cli-linux-amd64/);
    for (const name of HOMEBREW_PACKAGE_NAMES) assert.equal(existsSync(join(out, name)), false, name);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("rejects malformed checksum lines and invalid tags", () => {
  assert.throws(() => renderHomebrewPackages({ checksums: "not-a-hash  Ferryx_universal.dmg\n", tag: TAG }), /Malformed/);
  assert.throws(() => renderHomebrewPackages({ checksums: SUMS, tag: "2026.10.24" }), /Invalid release tag/);
});
