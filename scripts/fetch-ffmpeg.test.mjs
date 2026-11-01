import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { fetchSdk, isCurrent, main, parseArgs, readPin, requiredFiles, sdkKey } from "./fetch-ffmpeg.mjs";

const PIN = readPin();
const RECEIPT = `FFmpeg ${PIN.source.version} (sha256 ${PIN.source.sha256})\n--prefix=/x --enable-shared --disable-static --enable-vaapi\n`;

function withTmp(fn) {
  const dir = mkdtempSync(join(tmpdir(), "ferryx-fetch-ffmpeg-"));
  return Promise.resolve(fn(dir)).finally(() => rmSync(dir, { recursive: true, force: true }));
}

/** A tar.gz whose single top-level `ffmpeg/` dir holds `files` (relative path -> content). */
function sdkArchive(dir, files) {
  const root = join(dir, "src");
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(join(root, "ffmpeg", path, ".."), { recursive: true });
    writeFileSync(join(root, "ffmpeg", path), content);
  }
  const result = spawnSync("tar", ["-czf", "sdk.tar.gz", "-C", "src", "ffmpeg"], { cwd: dir, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  rmSync(root, { recursive: true, force: true });
  return readFileSync(join(dir, "sdk.tar.gz"));
}

function linuxFiles(receipt = RECEIPT) {
  return {
    "include/libavcodec/avcodec.h": "h",
    "include/libavutil/avutil.h": "h",
    "lib/libavcodec.so": "so",
    "lib/libavcodec.so.63": "so",
    "build-receipt.txt": receipt,
  };
}

/** The pin with the linux asset's sha256 replaced by that of `archive`. */
function pinFor(archive) {
  const pin = structuredClone(PIN);
  pin.sdk.assets["x86_64-unknown-linux-gnu"].sha256 = createHash("sha256").update(archive).digest("hex");
  return pin;
}

test("pin.json pins an SDK for linux x64, universal macOS and windows x64 from a ferryx prerelease", () => {
  assert.equal(PIN.sdk.repo, "Indosaram/ferryx");
  assert.match(PIN.sdk.tag, /^ffmpeg-sdk-9\.0\.2-\d+$/);
  for (const key of ["x86_64-unknown-linux-gnu", "universal-apple-darwin", "x86_64-pc-windows-msvc"]) {
    assert.match(PIN.sdk.assets[key].sha256, /^[0-9a-f]{64}$/, key);
    assert.equal(PIN.sdk.assets[key].name, `ffmpeg-lgpl-${PIN.source.version}-${key}.tar.gz`);
  }
});

test("both macOS triples use the universal SDK; unpinned triples point at FFMPEG_DIR", () => {
  assert.equal(sdkKey("aarch64-apple-darwin", PIN), "universal-apple-darwin");
  assert.equal(sdkKey("x86_64-apple-darwin", PIN), "universal-apple-darwin");
  assert.equal(sdkKey("x86_64-pc-windows-msvc", PIN), "x86_64-pc-windows-msvc");
  assert.throws(() => sdkKey("aarch64-unknown-linux-gnu", PIN), /set FFMPEG_DIR/);
});

test("parseArgs --from-env reads TAURI_ENV_TARGET_TRIPLE", () => {
  assert.equal(parseArgs(["--from-env"], { TAURI_ENV_TARGET_TRIPLE: "x86_64-pc-windows-msvc" }).target, "x86_64-pc-windows-msvc");
  assert.throws(() => parseArgs(["--bogus"]), /unknown argument/);
});

test("windows SDKs must carry the DLLs the dev binary loads", () => {
  assert.ok(requiredFiles("windows").includes("lib/avcodec.lib"));
  assert.ok(requiredFiles("windows").includes("bin/avcodec-*.dll"));
});

test("fetchSdk verifies the sha256 and receipt, installs, then skips a current install", () =>
  withTmp(async (dir) => {
    const archive = sdkArchive(dir, linuxFiles());
    const pin = pinFor(archive);
    const dest = join(dir, "resources", "sdk");
    const urls = [];
    const download = async (url) => {
      urls.push(url);
      return archive;
    };
    const first = await fetchSdk({ dest, triple: "x86_64-unknown-linux-gnu", pin, download, log: () => {} });
    assert.equal(first.skipped, false);
    assert.deepEqual(urls, [
      `https://github.com/Indosaram/ferryx/releases/download/${pin.sdk.tag}/ffmpeg-lgpl-9.0.2-x86_64-unknown-linux-gnu.tar.gz`,
    ]);
    assert.ok(existsSync(join(dest, "lib", "libavcodec.so")));
    assert.ok(isCurrent(dest, "x86_64-unknown-linux-gnu", pin));
    const second = await fetchSdk({ dest, triple: "x86_64-unknown-linux-gnu", pin, download, log: () => {} });
    assert.equal(second.skipped, true);
    assert.equal(urls.length, 1);
    // A changed pin is not trusted to the old install.
    const bumped = structuredClone(pin);
    bumped.sdk.tag = "ffmpeg-sdk-9.0.2-2";
    assert.equal(isCurrent(dest, "x86_64-unknown-linux-gnu", bumped), false);
  }));

test("a sha256 mismatch or a non-LGPL receipt installs nothing", () =>
  withTmp(async (dir) => {
    const dest = join(dir, "resources", "sdk");
    const good = sdkArchive(dir, linuxFiles());
    await assert.rejects(
      fetchSdk({ dest, triple: "x86_64-unknown-linux-gnu", pin: PIN, download: async () => good, log: () => {} }),
      /does not match the pinned/,
    );
    const gpl = sdkArchive(dir, linuxFiles(`${RECEIPT.trim()} --enable-gpl --enable-libx264\n`));
    await assert.rejects(
      fetchSdk({ dest, triple: "x86_64-unknown-linux-gnu", pin: pinFor(gpl), download: async () => gpl, log: () => {} }),
      /not LGPL-only/,
    );
    const incomplete = sdkArchive(dir, { "build-receipt.txt": RECEIPT });
    await assert.rejects(
      fetchSdk({ dest, triple: "x86_64-unknown-linux-gnu", pin: pinFor(incomplete), download: async () => incomplete, log: () => {} }),
      /lacks include\/libavcodec\/avcodec\.h/,
    );
    assert.equal(existsSync(dest), false);
  }));

test("an FFMPEG_DIR in the environment wins and nothing is downloaded", async () => {
  const lines = [];
  const result = await main([], {
    env: { FFMPEG_DIR: "/opt/ffmpeg" },
    download: async () => assert.fail("must not download"),
    log: (line) => lines.push(line),
  });
  assert.equal(result, null);
  assert.match(lines[0], /FFMPEG_DIR is set/);
});
