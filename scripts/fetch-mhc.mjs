#!/usr/bin/env bun
/**
 * Fetches the pinned mhc release and the pinned bun runtime for one or more target triples into
 * src-tauri/resources/mhc/payload, which tauri.conf.json bundles as the `mhc` resource dir.
 *
 * Both come from public GitHub releases (src-tauri/resources/mhc/pin.json):
 * - mhc: Indosaram/mhc, verified by sha256 (pinned) AND the release's minisign signature
 *   (pinned public key, trusted comment must name the pinned version).
 * - bun: oven-sh/bun, verified by sha256 (pinned) AND the release's SHASUMS256.txt line, so a
 *   changed upstream file fails in either direction. mhc's eval/bash kernel needs `bun` on PATH.
 *
 * Layout per triple:
 *   payload/<triple>/mhc/   the extracted mhc release folder (mhc[.exe], skills/, manifest.json, ...)
 *   payload/<triple>/bun/   bun[.exe] only, so the hub can prepend exactly this dir to PATH
 *   payload/<triple>/fetch.json  stamp of what was verified; a matching stamp skips the download
 *
 * Usage:
 *   bun scripts/fetch-mhc.mjs                  host triple
 *   bun scripts/fetch-mhc.mjs --target <triple|universal-apple-darwin> [--target ...]
 *   bun scripts/fetch-mhc.mjs --from-env       TAURI_ENV_TARGET_TRIPLE (tauri before* hooks), else host
 *   --prune   remove other triples from payload (release builds ship exactly their targets)
 *   --check   verify the stamps/files only; never download
 */
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { inflateRawSync } from "node:zlib";

import { parseSignature, verifyMinisign } from "./lib/minisign-verify.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const MHC_RESOURCE_DIR = join(REPO_ROOT, "src-tauri", "resources", "mhc");
export const PIN_PATH = join(MHC_RESOURCE_DIR, "pin.json");
export const DEFAULT_DEST = join(MHC_RESOURCE_DIR, "payload");
export const UNIVERSAL_DARWIN = "universal-apple-darwin";
const STAMP = "fetch.json";

export function readPin(path = PIN_PATH) {
  const pin = JSON.parse(readFileSync(path, "utf8"));
  if (pin.schemaVersion !== 1 || !pin.mhc?.assets || !pin.bun?.assets) {
    throw new Error(`unsupported mhc pin file ${path}`);
  }
  return pin;
}

export function hostTriple(platform = process.platform, arch = process.arch) {
  const cpu = { arm64: "aarch64", x64: "x86_64" }[arch];
  const os = { darwin: "apple-darwin", linux: "unknown-linux-gnu", win32: "pc-windows-msvc" }[platform];
  if (!cpu || !os) throw new Error(`no mhc release for host ${platform}/${arch}`);
  return `${cpu}-${os}`;
}

/** Expands `universal-apple-darwin` to both darwin triples and rejects triples without a pin. */
export function resolveTargets(requested, pin) {
  const triples = [];
  for (const target of requested) {
    const expanded = target === UNIVERSAL_DARWIN ? ["aarch64-apple-darwin", "x86_64-apple-darwin"] : [target];
    for (const triple of expanded) {
      if (!pin.mhc.assets[triple] || !pin.bun.assets[triple]) {
        throw new Error(`no pinned mhc/bun asset for target ${triple}`);
      }
      if (!triples.includes(triple)) triples.push(triple);
    }
  }
  return triples;
}

export function parseArgs(argv, env = process.env) {
  const options = { targets: [], prune: false, check: false, dest: DEFAULT_DEST };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--target") options.targets.push(argv[++i]);
    else if (arg === "--from-env") options.targets.push(env.TAURI_ENV_TARGET_TRIPLE || hostTriple());
    else if (arg === "--prune") options.prune = true;
    else if (arg === "--check") options.check = true;
    else if (arg === "--dest") options.dest = resolve(argv[++i]);
    else throw new Error(`unknown argument ${arg}`);
  }
  if (options.targets.some((target) => !target)) throw new Error("--target needs a value");
  if (options.targets.length === 0) options.targets.push(hostTriple());
  return options;
}

export function exeName(triple, name) {
  return triple.includes("windows") ? `${name}.exe` : name;
}

/** Paths of one triple's fetched layout under `dest`. */
export function layout(dest, triple) {
  const root = join(dest, triple);
  return {
    root,
    mhcDir: join(root, "mhc"),
    mhc: join(root, "mhc", exeName(triple, "mhc")),
    bunDir: join(root, "bun"),
    bun: join(root, "bun", exeName(triple, "bun")),
    stamp: join(root, STAMP),
  };
}

export function expectedStamp(pin, triple) {
  return {
    mhc: { tag: pin.mhc.tag, asset: pin.mhc.assets[triple].name, sha256: pin.mhc.assets[triple].sha256 },
    bun: { tag: pin.bun.tag, asset: pin.bun.assets[triple].name, sha256: pin.bun.assets[triple].sha256 },
  };
}

/** True when `dest/<triple>` holds exactly the pinned, previously verified fetch. */
export function isCurrent(dest, triple, pin) {
  const paths = layout(dest, triple);
  if (!existsSync(paths.mhc) || !existsSync(paths.bun) || !existsSync(join(paths.mhcDir, "manifest.json"))) {
    return false;
  }
  try {
    const stamp = JSON.parse(readFileSync(paths.stamp, "utf8"));
    return JSON.stringify(stamp) === JSON.stringify(expectedStamp(pin, triple));
  } catch {
    return false;
  }
}

export function sha256(buffer) {
  return createHash("sha256").update(buffer).digest("hex");
}

function releaseUrl(repo, tag, name) {
  return `https://github.com/${repo}/releases/download/${tag}/${name}`;
}

async function httpDownload(url) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) throw new Error(`GET ${url} failed: HTTP ${response.status}`);
  return Buffer.from(await response.arrayBuffer());
}

/** Verifies an mhc archive: pinned sha256, then minisign with the pinned key and version. */
export function verifyMhcArchive({ archive, signature, pin, triple }) {
  const asset = pin.mhc.assets[triple];
  const actual = sha256(archive);
  if (actual !== asset.sha256) {
    throw new Error(`${asset.name}: sha256 ${actual} does not match the pinned ${asset.sha256}`);
  }
  verifyMinisign({ data: archive, signature, publicKey: pin.mhc.publicKey });
  const version = pin.mhc.tag.replace(/^v/, "");
  const { trustedComment } = parseSignature(signature);
  if (!trustedComment.split(/\s+/).includes(version)) {
    throw new Error(`${asset.name}: signed trusted comment "${trustedComment}" does not name mhc ${version}`);
  }
  return { sha256: actual, trustedComment };
}

/** Verifies a bun zip against the pinned sha256 and the release's SHASUMS256.txt. */
export function verifyBunArchive({ archive, shasums, pin, triple }) {
  const asset = pin.bun.assets[triple];
  const actual = sha256(archive);
  if (actual !== asset.sha256) {
    throw new Error(`${asset.name}: sha256 ${actual} does not match the pinned ${asset.sha256}`);
  }
  const line = shasums
    .split(/\r?\n/)
    .map((text) => text.trim().split(/\s+\*?/))
    .find(([, name]) => name === asset.name);
  if (!line) throw new Error(`${asset.name}: missing from ${pin.bun.tag} SHASUMS256.txt`);
  if (line[0].toLowerCase() !== asset.sha256) {
    throw new Error(`${asset.name}: SHASUMS256.txt lists ${line[0]}, pinned ${asset.sha256}`);
  }
  return { sha256: actual };
}

/**
 * Extracts a zip (stored or deflated entries, no ZIP64) into `dest`, refusing entries that
 * escape it. Entries named in `executables` (relative paths) get mode 0755.
 */
export function extractZip(buffer, dest, executables = () => false) {
  let eocd = -1;
  for (let i = buffer.length - 22; i >= Math.max(0, buffer.length - 22 - 0xffff); i -= 1) {
    if (buffer.readUInt32LE(i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) throw new Error("zip: end of central directory not found");
  const entries = buffer.readUInt16LE(eocd + 10);
  let offset = buffer.readUInt32LE(eocd + 16);
  if (entries === 0xffff || offset === 0xffffffff) throw new Error("zip: ZIP64 archives are not supported");
  const root = resolve(dest);
  for (let n = 0; n < entries; n += 1) {
    if (buffer.readUInt32LE(offset) !== 0x02014b50) throw new Error("zip: bad central directory entry");
    const method = buffer.readUInt16LE(offset + 10);
    const compressedSize = buffer.readUInt32LE(offset + 20);
    const nameLength = buffer.readUInt16LE(offset + 28);
    const extraLength = buffer.readUInt16LE(offset + 30);
    const commentLength = buffer.readUInt16LE(offset + 32);
    const localOffset = buffer.readUInt32LE(offset + 42);
    const name = buffer.toString("utf8", offset + 46, offset + 46 + nameLength).replace(/\\/g, "/");
    offset += 46 + nameLength + extraLength + commentLength;

    const target = resolve(root, name);
    if (target !== root && !target.startsWith(root + sep)) throw new Error(`zip: entry ${name} escapes the destination`);
    if (name.endsWith("/")) {
      mkdirSync(target, { recursive: true });
      continue;
    }
    const start = localOffset + 30 + buffer.readUInt16LE(localOffset + 26) + buffer.readUInt16LE(localOffset + 28);
    const data = buffer.subarray(start, start + compressedSize);
    let content;
    if (method === 0) content = data;
    else if (method === 8) content = inflateRawSync(data);
    else throw new Error(`zip: entry ${name} uses unsupported method ${method}`);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, content);
    if (executables(name)) chmodSync(target, 0o755);
  }
}

function extractTarGz(archivePath, dest) {
  mkdirSync(dest, { recursive: true });
  const result = spawnSync("tar", ["-xzf", archivePath, "-C", dest], { encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`tar -xzf ${archivePath} failed: ${result.stderr.trim()}`);
}

/** The single top-level dir of an extracted archive. */
function singleTopDir(dir) {
  const entries = readdirSync(dir).filter((name) => !name.startsWith("."));
  if (entries.length !== 1 || !statSync(join(dir, entries[0])).isDirectory()) {
    throw new Error(`expected one top-level folder in the archive, found ${JSON.stringify(entries)}`);
  }
  return join(dir, entries[0]);
}

/** Checks every executable the mhc release manifest lists against its recorded sha256. */
export function verifyMhcManifest(mhcDir, triple) {
  const manifest = JSON.parse(readFileSync(join(mhcDir, "manifest.json"), "utf8"));
  const binary = manifest.layout?.binary;
  if (binary !== "mhc") throw new Error(`mhc manifest layout.binary is ${JSON.stringify(binary)}, expected "mhc"`);
  for (const executable of manifest.executables ?? []) {
    const file = join(mhcDir, executable.path);
    const actual = sha256(readFileSync(file));
    if (actual !== executable.sha256) {
      throw new Error(`mhc manifest: ${executable.path} sha256 ${actual} != ${executable.sha256}`);
    }
    if (!triple.includes("windows")) chmodSync(file, 0o755);
  }
  if (!existsSync(join(mhcDir, exeName(triple, "mhc")))) throw new Error(`mhc release lacks ${exeName(triple, "mhc")}`);
  return manifest;
}

/** Downloads, verifies and installs one triple. `download(url)` returns a Buffer. */
export async function fetchTriple({ dest, triple, pin, download = httpDownload, log = console.log }) {
  const paths = layout(dest, triple);
  if (isCurrent(dest, triple, pin)) {
    log(`mhc ${pin.mhc.tag} + bun ${pin.bun.tag} for ${triple}: already present and verified`);
    return { skipped: true, paths };
  }
  const mhcAsset = pin.mhc.assets[triple];
  const bunAsset = pin.bun.assets[triple];
  const archive = await download(releaseUrl(pin.mhc.repo, pin.mhc.tag, mhcAsset.name));
  const signature = (await download(releaseUrl(pin.mhc.repo, pin.mhc.tag, `${mhcAsset.name}.minisig`))).toString("utf8");
  const mhcCheck = verifyMhcArchive({ archive, signature, pin, triple });
  log(`${mhcAsset.name}: sha256 ${mhcCheck.sha256} matches pin; minisign signature verified ("${mhcCheck.trustedComment}")`);
  const bunZip = await download(releaseUrl(pin.bun.repo, pin.bun.tag, bunAsset.name));
  const shasums = (await download(releaseUrl(pin.bun.repo, pin.bun.tag, "SHASUMS256.txt"))).toString("utf8");
  const bunCheck = verifyBunArchive({ archive: bunZip, shasums, pin, triple });
  log(`${bunAsset.name}: sha256 ${bunCheck.sha256} matches pin and ${pin.bun.tag} SHASUMS256.txt`);

  mkdirSync(dest, { recursive: true });
  const staging = join(dest, `.staging-${triple}-${process.pid}`);
  rmSync(staging, { recursive: true, force: true });
  try {
    const mhcExtract = join(staging, "mhc-extract");
    if (mhcAsset.name.endsWith(".zip")) {
      extractZip(archive, mhcExtract);
    } else {
      mkdirSync(staging, { recursive: true });
      const archivePath = join(staging, mhcAsset.name);
      writeFileSync(archivePath, archive);
      extractTarGz(archivePath, mhcExtract);
    }
    const bunExtract = join(staging, "bun-extract");
    extractZip(bunZip, bunExtract, (name) => /(^|\/)bun(\.exe)?$/.test(name));

    const tripleDir = join(staging, "out");
    mkdirSync(join(tripleDir, "bun"), { recursive: true });
    renameSync(singleTopDir(mhcExtract), join(tripleDir, "mhc"));
    verifyMhcManifest(join(tripleDir, "mhc"), triple);
    const bunName = exeName(triple, "bun");
    renameSync(join(singleTopDir(bunExtract), bunName), join(tripleDir, "bun", bunName));
    writeFileSync(join(tripleDir, STAMP), `${JSON.stringify(expectedStamp(pin, triple), null, 2)}\n`);

    rmSync(paths.root, { recursive: true, force: true });
    renameSync(tripleDir, paths.root);
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
  log(`installed ${triple} into ${paths.root}`);
  return { skipped: false, paths };
}

/** Removes every triple dir in `dest` that is not in `keep` (keeps dotfiles such as .gitkeep). */
export function pruneOthers(dest, keep) {
  if (!existsSync(dest)) return [];
  const removed = [];
  for (const name of readdirSync(dest)) {
    if (name.startsWith(".") || keep.includes(name)) continue;
    rmSync(join(dest, name), { recursive: true, force: true });
    removed.push(name);
  }
  return removed;
}

export async function main(argv = process.argv.slice(2), { download = httpDownload, log = console.log } = {}) {
  const options = parseArgs(argv);
  const pin = readPin();
  const triples = resolveTargets(options.targets, pin);
  if (options.check) {
    const missing = triples.filter((triple) => !isCurrent(options.dest, triple, pin));
    if (missing.length > 0) {
      throw new Error(`bundled mhc missing or stale for ${missing.join(", ")}; run: bun scripts/fetch-mhc.mjs --target <triple>`);
    }
    log(`bundled mhc ${pin.mhc.tag} + bun ${pin.bun.tag} present for ${triples.join(", ")}`);
    return triples;
  }
  for (const triple of triples) {
    await fetchTriple({ dest: options.dest, triple, pin, download, log });
  }
  if (options.prune) {
    for (const name of pruneOthers(options.dest, triples)) log(`pruned ${name} from ${options.dest}`);
  }
  return triples;
}

const invokedDirectly = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) {
  main().catch((error) => {
    console.error(`fetch-mhc: ${error.message}`);
    process.exit(1);
  });
}
