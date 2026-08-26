import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(REPO_ROOT, "scripts", "assert-updater-archive-layout.mjs");

function archiveFixture(withAppleDouble) {
  const dir = mkdtempSync(join(tmpdir(), "updater-archive-layout-"));
  const app = join(dir, "Ferryx.app");
  mkdirSync(join(app, "Contents"), { recursive: true });
  writeFileSync(join(app, "Contents", "Info.plist"), "fixture");
  if (withAppleDouble) writeFileSync(join(dir, "._Ferryx.app"), "metadata");

  const archive = join(dir, "Ferryx.app.tar.gz");
  execFileSync("tar", ["-czf", archive, "Ferryx.app", ...(withAppleDouble ? ["._Ferryx.app"] : [])], {
    cwd: dir,
    env: withAppleDouble ? process.env : { ...process.env, COPYFILE_DISABLE: "1" },
  });
  return { archive, dir };
}

function inspect(archive) {
  return spawnSync(process.execPath, [SCRIPT, archive], { encoding: "utf8" });
}

test("rejects updater archives containing a top-level AppleDouble entry", () => {
  const { archive, dir } = archiveFixture(true);
  try {
    const result = inspect(archive);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /AppleDouble.*\._Ferryx\.app/i);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("accepts updater archives whose entries are rooted in the app bundle", () => {
  const { archive, dir } = archiveFixture(false);
  try {
    const result = inspect(archive);
    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
