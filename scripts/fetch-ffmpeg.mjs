#!/usr/bin/env bun
/**
 * Fetches the pinned, prebuilt LGPL FFmpeg 9 SDK (headers, link libraries, shared libraries) that
 * the default `remote-desktop` feature links against, into src-tauri/resources/ffmpeg/sdk
 * (gitignored). The repository's .cargo/config.toml points FFMPEG_DIR there, so a plain
 * `cargo build`, `cargo test` or `cargo tauri dev` finds it without a system FFmpeg.
 *
 * The SDK archives are public prerelease assets of Indosaram/ferryx (src-tauri/resources/ffmpeg/
 * pin.json, `sdk`), built from the pinned source tarball by scripts/build-ffmpeg-lgpl.sh without
 * GPL, nonfree, x264, x265 or OpenH264 code. Each download must match its pinned sha256, and the
 * extracted build-receipt.txt must name the pinned tarball and an LGPL-only configuration.
 * Release builds do not use these SDKs: they build FFmpeg on the release host and set FFMPEG_DIR.
 *
 * Usage:
 *   bun scripts/fetch-ffmpeg.mjs                 the SDK for this machine
 *   bun scripts/fetch-ffmpeg.mjs --target <triple>
 *   bun scripts/fetch-ffmpeg.mjs --from-env      TAURI_ENV_TARGET_TRIPLE (tauri before* hooks), else host
 *   --check   verify the installed SDK only; never download
 *
 * When FFMPEG_DIR is already set in the environment (your own FFmpeg 9 build, or a release host),
 * the script does nothing: cargo uses that value instead of the fetched SDK.
 */
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { hostTriple, sha256 } from "./fetch-mhc.mjs";
import { FFMPEG_RESOURCE_DIR, assertLgplReceipt, osOf, readPin as readFfmpegPin } from "./stage-ffmpeg.mjs";

export const DEFAULT_DEST = join(FFMPEG_RESOURCE_DIR, "sdk");
const STAMP = "fetch.json";

export function readPin(path) {
  const pin = readFfmpegPin(path);
  if (!pin.sdk?.repo || !pin.sdk?.tag || !pin.sdk?.assets) throw new Error("src-tauri/resources/ffmpeg/pin.json has no `sdk` section");
  return pin;
}

/** The pinned SDK asset key for a target triple: both macOS triples share the universal SDK. */
export function sdkKey(triple, pin) {
  const key = triple.endsWith("-apple-darwin") ? "universal-apple-darwin" : triple;
  if (!pin.sdk.assets[key]) {
    throw new Error(
      `no prebuilt FFmpeg SDK for ${triple}; build one with scripts/build-ffmpeg-lgpl.sh and set FFMPEG_DIR to its prefix`,
    );
  }
  return key;
}

export function parseArgs(argv, env = process.env) {
  const options = { target: null, check: false, dest: DEFAULT_DEST };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--target") options.target = argv[++i];
    else if (arg === "--from-env") options.target = env.TAURI_ENV_TARGET_TRIPLE || hostTriple();
    else if (arg === "--check") options.check = true;
    else if (arg === "--dest") options.dest = resolve(argv[++i]);
    else throw new Error(`unknown argument ${arg}`);
  }
  if (options.target === undefined || options.target === "") throw new Error("--target needs a value");
  options.target ??= hostTriple();
  return options;
}

export function expectedStamp(pin, key) {
  const asset = pin.sdk.assets[key];
  return { tag: pin.sdk.tag, asset: asset.name, sha256: asset.sha256 };
}

/** The files a linkable SDK for `os` must contain, relative to its root. */
export function requiredFiles(os) {
  const lib = { linux: "lib/libavcodec.so", darwin: "lib/libavcodec.dylib", windows: "lib/avcodec.lib" }[os];
  const files = ["include/libavcodec/avcodec.h", "include/libavutil/avutil.h", "build-receipt.txt", lib];
  if (os === "windows") files.push(...["avcodec", "avformat", "avutil", "swscale"].map((name) => `bin/${name}-*.dll`));
  return files;
}

function present(root, pattern) {
  if (!pattern.includes("*")) return existsSync(join(root, pattern));
  const dir = join(root, dirname(pattern));
  const [prefix, suffix] = pattern.slice(dirname(pattern).length + 1).split("*");
  return existsSync(dir) && readdirSync(dir).some((name) => name.startsWith(prefix) && name.endsWith(suffix));
}

/** Throws unless `root` is a complete LGPL-only SDK built from the pinned source. */
export function verifySdk(root, os, pin) {
  const missing = requiredFiles(os).filter((file) => !present(root, file));
  if (missing.length > 0) throw new Error(`FFmpeg SDK at ${root} lacks ${missing.join(", ")}`);
  assertLgplReceipt(readFileSync(join(root, "build-receipt.txt"), "utf8"), {
    source: pin.source,
    label: join(root, "build-receipt.txt"),
  });
}

/** True when `dest` holds exactly the pinned SDK, already verified. */
export function isCurrent(dest, key, pin) {
  try {
    const stamp = JSON.parse(readFileSync(join(dest, STAMP), "utf8"));
    if (JSON.stringify(stamp) !== JSON.stringify(expectedStamp(pin, key))) return false;
    verifySdk(dest, osOf(key), pin);
    return true;
  } catch {
    return false;
  }
}

async function httpDownload(url) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) throw new Error(`GET ${url} failed: HTTP ${response.status}`);
  return Buffer.from(await response.arrayBuffer());
}

/** Extracts with the system tar from inside `cwd`, so no path holds a drive letter on Windows. */
function extractTarGz(cwd, archiveName, outName) {
  mkdirSync(join(cwd, outName), { recursive: true });
  const result = spawnSync("tar", ["-xzf", archiveName, "-C", outName], { cwd, encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`tar -xzf ${archiveName} failed: ${result.stderr.trim()}`);
}

/** Downloads, verifies and installs the SDK for `triple` into `dest`. */
export async function fetchSdk({ dest, triple, pin, download = httpDownload, log = console.log }) {
  const key = sdkKey(triple, pin);
  if (isCurrent(dest, key, pin)) {
    log(`FFmpeg SDK ${pin.sdk.tag} (${key}): already present and verified at ${dest}`);
    return { skipped: true, key };
  }
  const asset = pin.sdk.assets[key];
  const url = `https://github.com/${pin.sdk.repo}/releases/download/${pin.sdk.tag}/${asset.name}`;
  const archive = await download(url);
  const actual = sha256(archive);
  if (actual !== asset.sha256) throw new Error(`${asset.name}: sha256 ${actual} does not match the pinned ${asset.sha256}`);
  log(`${asset.name}: sha256 matches pin`);

  const parent = dirname(dest);
  mkdirSync(parent, { recursive: true });
  const stagingName = `.staging-sdk-${process.pid}`;
  const staging = join(parent, stagingName);
  rmSync(staging, { recursive: true, force: true });
  mkdirSync(staging, { recursive: true });
  try {
    writeFileSync(join(staging, asset.name), archive);
    extractTarGz(staging, asset.name, "out");
    const root = join(staging, "out", "ffmpeg");
    verifySdk(root, osOf(key), pin);
    writeFileSync(join(root, STAMP), `${JSON.stringify(expectedStamp(pin, key), null, 2)}\n`);
    rmSync(dest, { recursive: true, force: true });
    renameSync(root, dest);
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
  log(`installed FFmpeg SDK ${pin.sdk.tag} (${key}) into ${dest}`);
  return { skipped: false, key };
}

export async function main(argv = process.argv.slice(2), { env = process.env, download = httpDownload, log = console.log } = {}) {
  if (env.FFMPEG_DIR) {
    log(`FFMPEG_DIR is set (${env.FFMPEG_DIR}); cargo links that FFmpeg, nothing to fetch`);
    return null;
  }
  const options = parseArgs(argv, env);
  const pin = readPin();
  const key = sdkKey(options.target, pin);
  if (options.check) {
    if (!isCurrent(options.dest, key, pin)) {
      throw new Error(`FFmpeg SDK missing or stale at ${options.dest}; run: bun scripts/fetch-ffmpeg.mjs`);
    }
    log(`FFmpeg SDK ${pin.sdk.tag} (${key}) present and verified`);
    return key;
  }
  await fetchSdk({ dest: options.dest, triple: options.target, pin, download, log });
  return key;
}

const invokedDirectly = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) {
  main().catch((error) => {
    console.error(`fetch-ffmpeg: ${error.message}`);
    process.exit(1);
  });
}
