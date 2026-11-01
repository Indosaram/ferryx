#!/usr/bin/env bun
/**
 * Stages the LGPL FFmpeg 9 shared libraries that a `--features remote-desktop` build of Ferryx
 * loads at runtime, into src-tauri/resources/ffmpeg/payload/<triple> (gitignored).
 *
 * Source: on every OS a prefix made by scripts/build-ffmpeg-lgpl.sh from the tarball pinned in
 * src-tauri/resources/ffmpeg/pin.json. Its build-receipt.txt must name that tarball and an
 * LGPL-only configuration without OpenH264. macOS universal takes one prefix per slice and
 * lipo's them. Windows takes the DLLs from bin/ and the headers + MSVC import libraries as sdk/.
 *
 * Layout of payload/<triple>:
 *   runtime/                   the libraries the app loads (dylib / .so.N / .dll)
 *   legal/                     license texts, build-receipt.txt, SOURCE.txt (source offer)
 *   sdk/                       Windows only: include/ + import libs for FFMPEG_DIR at link time
 *   stage.json                 sha256 of every runtime and legal file
 *   tauri.ffmpeg.conf.json     `tauri build -c` overlay that bundles runtime/ and legal/
 *
 * Releases without the feature never run this script and never pass the overlay, so their bundles
 * are unchanged. `--check` re-verifies a staged payload and fails closed on any mismatch.
 *
 * Usage:
 *   bun scripts/stage-ffmpeg.mjs --target <triple|universal-apple-darwin> [--prefix [<triple>=]<dir>]...
 *   bun scripts/stage-ffmpeg.mjs --target <triple> --check
 *   --prune   remove other triples from the payload dir
 */
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { hostTriple, sha256 } from "./fetch-mhc.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const TAURI_DIR = join(REPO_ROOT, "src-tauri");
export const FFMPEG_RESOURCE_DIR = join(TAURI_DIR, "resources", "ffmpeg");
export const PIN_PATH = join(FFMPEG_RESOURCE_DIR, "pin.json");
export const DEFAULT_DEST = join(FFMPEG_RESOURCE_DIR, "payload");
export const UNIVERSAL_DARWIN = "universal-apple-darwin";
export const OVERLAY_NAME = "tauri.ffmpeg.conf.json";
export const MANIFEST_NAME = "stage.json";
/** Install-relative dir of the Linux libraries; build.rs bakes `$ORIGIN/../lib/Ferryx/ffmpeg`. */
export const LINUX_LIB_DIR = "ffmpeg";
export const LIBRARIES = Object.freeze(["avutil", "swresample", "swscale", "avcodec", "avformat", "avfilter", "avdevice"]);
/** Runtime packages the VAAPI-enabled Linux libraries need (deb `depends`). */
export const LINUX_DEB_DEPENDS = Object.freeze(["libva2", "libva-drm2"]);
const FORBIDDEN_FLAGS = Object.freeze([
  "--enable-gpl",
  "--enable-nonfree",
  "--enable-libx264",
  "--enable-libx265",
  "--enable-libxvid",
  "--enable-libfdk-aac",
  // Cisco's patent licence covers only Cisco's own binary; no shipped payload may contain it.
  "--enable-libopenh264",
]);
const SUPPORTED = Object.freeze([
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  UNIVERSAL_DARWIN,
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "x86_64-pc-windows-msvc",
]);

export function readPin(path = PIN_PATH) {
  const pin = JSON.parse(readFileSync(path, "utf8"));
  if (pin.schemaVersion !== 1 || !pin.source?.sha256 || !pin.source?.url) throw new Error(`unsupported FFmpeg pin file ${path}`);
  return pin;
}

export function osOf(triple) {
  if (triple.endsWith("apple-darwin")) return "darwin";
  if (triple.includes("linux")) return "linux";
  if (triple.includes("windows")) return "windows";
  throw new Error(`unsupported FFmpeg target ${triple}`);
}

/** True for the versioned file the loader resolves: libavcodec.63.dylib, libavcodec.so.63, avcodec-63.dll. */
export function isRuntimeLibrary(triple, name) {
  const names = LIBRARIES.join("|");
  const pattern = {
    darwin: new RegExp(`^lib(${names})\\.\\d+\\.dylib$`),
    linux: new RegExp(`^lib(${names})\\.so\\.\\d+$`),
    windows: new RegExp(`^(${names})-\\d+\\.dll$`),
  }[osOf(triple)];
  return pattern.test(name);
}

/**
 * Throws unless `text` is an LGPL-only FFmpeg configuration. With `source`, the receipt's first line
 * must name exactly that version and tarball sha256 (receipts from scripts/build-ffmpeg-lgpl.sh).
 */
export function assertLgplReceipt(text, { source = null, label = "build-receipt.txt" } = {}) {
  if (typeof text !== "string" || text.trim() === "") throw new Error(`${label}: missing or empty`);
  if (source) {
    const want = `FFmpeg ${source.version} (sha256 ${source.sha256})`;
    const sections = text.split(/^# slice .*$/m).map((part) => part.trim()).filter(Boolean);
    for (const section of sections) {
      if (section.split("\n")[0].trim() !== want) throw new Error(`${label}: does not name the pinned source "${want}"`);
    }
  }
  if (!/--enable-shared\b/.test(text)) throw new Error(`${label}: configuration lacks --enable-shared`);
  for (const flag of FORBIDDEN_FLAGS) {
    if (new RegExp(`${flag}(\\s|$)`, "m").test(text)) throw new Error(`${label}: not LGPL-only (${flag})`);
  }
  return true;
}

function defaultExec(command, args) {
  const res = spawnSync(command, args, { encoding: "utf8" });
  if (res.error) throw new Error(`${command} failed to start: ${res.error.message}`);
  if (res.status !== 0) throw new Error(`${command} ${args.join(" ")} failed: ${(res.stderr || res.stdout).trim()}`);
  return res.stdout;
}

export function parseArgs(argv, env = process.env) {
  const options = { targets: [], prefixes: {}, check: false, prune: false, dest: DEFAULT_DEST };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--target") options.targets.push(argv[++i]);
    else if (arg === "--from-env") options.targets.push(env.TAURI_ENV_TARGET_TRIPLE || hostTriple());
    else if (arg === "--prefix") {
      const value = argv[++i];
      if (!value) throw new Error("--prefix needs a value");
      const eq = value.indexOf("=");
      const isKeyed = eq > 0 && SUPPORTED.includes(value.slice(0, eq));
      options.prefixes[isKeyed ? value.slice(0, eq) : "*"] = resolve(isKeyed ? value.slice(eq + 1) : value);
    } else if (arg === "--check") options.check = true;
    else if (arg === "--prune") options.prune = true;
    else if (arg === "--dest") options.dest = resolve(argv[++i]);
    else throw new Error(`unknown argument ${arg}`);
  }
  if (options.targets.some((target) => !target)) throw new Error("--target needs a value");
  if (options.targets.length === 0) options.targets.push(hostTriple());
  for (const target of options.targets) {
    if (!SUPPORTED.includes(target)) throw new Error(`unsupported FFmpeg target ${target}`);
  }
  return options;
}

function listFiles(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).filter((name) => statSync(join(dir, name)).isFile()).sort();
}

function hashDir(dir) {
  return Object.fromEntries(listFiles(dir).map((name) => [name, sha256(readFileSync(join(dir, name)))]));
}

/** The `tauri build -c` overlay for one staged triple; paths are relative to src-tauri. */
export function overlayConfig({ triple, root, runtime, legal, baseConfig, tauriDir = TAURI_DIR }) {
  const rel = (...parts) => relative(tauriDir, join(root, ...parts)).split("\\").join("/");
  const resources = {};
  for (const name of legal) resources[rel("legal", name)] = `ffmpeg-legal/${name}`;
  const bundle = { resources };
  const os = osOf(triple);
  if (os === "darwin") {
    // Copied into Contents/Frameworks; the binary carries @executable_path/../Frameworks (build.rs).
    bundle.macOS = { frameworks: runtime.map((name) => join(root, "runtime", name)) };
  } else if (os === "windows") {
    // The resource dir is the install dir: the DLLs land beside ferryx.exe.
    for (const name of runtime) resources[rel("runtime", name)] = name;
  } else {
    for (const name of runtime) resources[rel("runtime", name)] = `${LINUX_LIB_DIR}/${name}`;
    const depends = [...(baseConfig?.bundle?.linux?.deb?.depends ?? [])];
    for (const dep of LINUX_DEB_DEPENDS) if (!depends.includes(dep)) depends.push(dep);
    bundle.linux = { deb: { depends } };
  }
  return { bundle };
}

function sourceText(pin) {
  const lines = [
    "Ferryx ships FFmpeg as unmodified shared libraries under the LGPL (see THIRD_PARTY_NOTICES.md).",
    "You may replace them with your own build of the same major version; the app loads them dynamically.",
    "Corresponding source:",
  ];
  lines.push(`  ${pin.source.url} (sha256 ${pin.source.sha256})`);
  lines.push("  Built with scripts/build-ffmpeg-lgpl.sh from the Ferryx source tree; flags in build-receipt.txt.");
  lines.push("Written offer: for three years from the release date the Ferryx maintainers will provide this");
  lines.push("source on request through https://github.com/Indosaram/ferryx/issues.");
  return `${lines.join("\n")}\n`;
}

function copyLegal(fromDir, legalDir) {
  if (!existsSync(fromDir)) return;
  for (const name of listFiles(fromDir)) copyFileSync(join(fromDir, name), join(legalDir, name));
}

/** Copies the runtime libraries of one prefix (resolving symlinks) and fixes their load paths. */
function stagePrefixLibraries({ triple, prefix, runtimeDir, exec }) {
  // MSVC installs the DLLs in bin/, Unix builds the shared objects in lib/.
  const libDir = join(prefix, osOf(triple) === "windows" ? "bin" : "lib");
  const names = existsSync(libDir) ? readdirSync(libDir).filter((name) => isRuntimeLibrary(triple, name)).sort() : [];
  if (names.length < 4) throw new Error(`${libDir}: expected the FFmpeg shared libraries, found ${JSON.stringify(names)}`);
  for (const name of names) copyFileSync(join(libDir, name), join(runtimeDir, name));
  if (osOf(triple) === "linux") {
    for (const name of names) exec("patchelf", ["--set-rpath", "$ORIGIN", join(runtimeDir, name)]);
  }
  return names;
}

function fixDarwinInstallNames(runtimeDir, names, exec) {
  for (const name of names) {
    const path = join(runtimeDir, name);
    const args = ["-id", `@rpath/${name}`];
    const deps = exec("otool", ["-L", path]).split("\n").slice(1).map((line) => line.trim().split(" ")[0]).filter(Boolean);
    for (const dep of deps) {
      if (names.includes(basename(dep)) && dep !== `@rpath/${basename(dep)}`) args.push("-change", dep, `@rpath/${basename(dep)}`);
    }
    exec("install_name_tool", [...args, path]);
  }
}

/** Windows link-time SDK: headers and MSVC import libraries, used as FFMPEG_DIR by ffmpeg-sys-next. */
function copyWindowsSdk(prefix, sdkDir) {
  const libs = existsSync(join(prefix, "lib")) ? readdirSync(join(prefix, "lib")).filter((name) => /^(avutil|swresample|swscale|avcodec|avformat|avfilter|avdevice)\.lib$/.test(name)) : [];
  if (libs.length < 4 || !existsSync(join(prefix, "include", "libavutil"))) {
    throw new Error(`${prefix}: expected include/ and MSVC import libraries (lib/*.lib); rebuild with the current scripts/build-ffmpeg-lgpl.sh`);
  }
  mkdirSync(join(sdkDir, "lib"), { recursive: true });
  for (const name of libs) copyFileSync(join(prefix, "lib", name), join(sdkDir, "lib", name));
  cpSync(join(prefix, "include"), join(sdkDir, "include"), { recursive: true });
}

/** Stages a target from build-ffmpeg-lgpl.sh prefixes. */
export function stageFromPrefixes({ triple, prefixes, out, pin, exec = defaultExec }) {
  const slices = triple === UNIVERSAL_DARWIN ? ["aarch64-apple-darwin", "x86_64-apple-darwin"] : [triple];
  const receipts = [];
  const runtimeDir = join(out, "runtime");
  const legalDir = join(out, "legal");
  mkdirSync(runtimeDir, { recursive: true });
  mkdirSync(legalDir, { recursive: true });
  const sliceDirs = [];
  for (const slice of slices) {
    const prefix = prefixes[slice] ?? (slices.length === 1 ? prefixes["*"] : undefined);
    if (!prefix) throw new Error(`no FFmpeg prefix for ${slice}; pass --prefix ${slice}=<dir> (scripts/build-ffmpeg-lgpl.sh output)`);
    const receiptPath = join(prefix, "build-receipt.txt");
    if (!existsSync(receiptPath)) throw new Error(`${receiptPath} is missing; build the prefix with scripts/build-ffmpeg-lgpl.sh`);
    const receipt = readFileSync(receiptPath, "utf8");
    assertLgplReceipt(receipt, { source: pin.source, label: receiptPath });
    receipts.push(slices.length > 1 ? `# slice ${slice}\n${receipt}` : receipt);
    const sliceDir = slices.length > 1 ? join(out, `.slice-${slice}`) : runtimeDir;
    mkdirSync(sliceDir, { recursive: true });
    sliceDirs.push({ dir: sliceDir, names: stagePrefixLibraries({ triple: slice, prefix, runtimeDir: sliceDir, exec }) });
    copyLegal(join(prefix, "share", "ferryx-legal"), legalDir);
    if (osOf(slice) === "windows") copyWindowsSdk(prefix, join(out, "sdk"));
  }
  let names = sliceDirs[0].names;
  if (sliceDirs.length > 1) {
    const [arm, x86] = sliceDirs;
    if (arm.names.join() !== x86.names.join()) throw new Error(`universal slices differ: ${arm.names} vs ${x86.names}`);
    for (const name of names) exec("lipo", ["-create", join(arm.dir, name), join(x86.dir, name), "-output", join(runtimeDir, name)]);
    for (const slice of sliceDirs) rmSync(slice.dir, { recursive: true, force: true });
  }
  if (osOf(triple) === "darwin") fixDarwinInstallNames(runtimeDir, names, exec);
  for (const required of ["COPYING.LGPLv2.1", "LICENSE.md"]) {
    if (!existsSync(join(legalDir, required))) throw new Error(`prefix lacks share/ferryx-legal/${required}; rebuild with the current scripts/build-ffmpeg-lgpl.sh`);
  }
  writeFileSync(join(legalDir, "build-receipt.txt"), receipts.join("\n"));
  writeFileSync(join(legalDir, "SOURCE.txt"), sourceText(pin));
  return names;
}

/** Re-verifies a staged payload: receipt is LGPL-only, every file matches stage.json, nothing extra. */
export function checkStaged({ dest, triple, pin }) {
  const root = join(dest, triple);
  const manifestPath = join(root, MANIFEST_NAME);
  if (!existsSync(manifestPath)) {
    throw new Error(`FFmpeg payload for ${triple} is not staged (${manifestPath} missing); run: bun scripts/stage-ffmpeg.mjs --target ${triple} --prefix ...`);
  }
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  if (manifest.schemaVersion !== 1 || manifest.triple !== triple) throw new Error(`${manifestPath}: wrong schema or triple`);
  if (manifest.source?.sha256 !== pin.source.sha256) {
    throw new Error(`${manifestPath}: staged from a different FFmpeg than src-tauri/resources/ffmpeg/pin.json`);
  }
  for (const part of ["runtime", "legal"]) {
    const actual = hashDir(join(root, part));
    if (JSON.stringify(actual) !== JSON.stringify(manifest[part])) {
      throw new Error(`FFmpeg payload ${triple}/${part} does not match ${MANIFEST_NAME} (hash or file set changed)`);
    }
  }
  const runtime = Object.keys(manifest.runtime);
  if (runtime.length < 4 || !runtime.every((name) => isRuntimeLibrary(triple, name))) throw new Error(`${manifestPath}: bad runtime set`);
  for (const name of ["build-receipt.txt", "SOURCE.txt"]) {
    if (!manifest.legal[name]) throw new Error(`${manifestPath}: legal/${name} missing`);
  }
  assertLgplReceipt(readFileSync(join(root, "legal", "build-receipt.txt"), "utf8"), {
    source: pin.source,
    label: `${triple}/legal/build-receipt.txt`,
  });
  if (osOf(triple) === "windows" && !existsSync(join(root, "sdk", "include", "libavutil"))) {
    throw new Error(`${triple}/sdk (headers + import libraries for FFMPEG_DIR) missing`);
  }
  if (!existsSync(join(root, OVERLAY_NAME))) throw new Error(`${triple}/${OVERLAY_NAME} missing`);
  return manifest;
}

/** Stages one target into dest/<triple> atomically, then verifies it. */
export async function stageTarget({ dest, triple, pin, prefixes = {}, exec = defaultExec }) {
  mkdirSync(dest, { recursive: true });
  const staging = join(dest, `.staging-${triple}-${process.pid}`);
  rmSync(staging, { recursive: true, force: true });
  mkdirSync(staging, { recursive: true });
  const root = join(dest, triple);
  try {
    stageFromPrefixes({ triple, prefixes, out: staging, pin, exec });
    const runtime = hashDir(join(staging, "runtime"));
    const legal = hashDir(join(staging, "legal"));
    const source = { kind: "source", version: pin.source.version, url: pin.source.url, sha256: pin.source.sha256 };
    const baseConfig = JSON.parse(readFileSync(join(TAURI_DIR, "tauri.conf.json"), "utf8"));
    const overlay = overlayConfig({ triple, root, runtime: Object.keys(runtime), legal: Object.keys(legal), baseConfig });
    writeFileSync(join(staging, OVERLAY_NAME), `${JSON.stringify(overlay, null, 2)}\n`);
    writeFileSync(join(staging, MANIFEST_NAME), `${JSON.stringify({ schemaVersion: 1, triple, source, runtime, legal }, null, 2)}\n`);
    rmSync(root, { recursive: true, force: true });
    renameSync(staging, root);
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
  return checkStaged({ dest, triple, pin });
}

export async function main(argv = process.argv.slice(2), { log = console.log, exec = defaultExec } = {}) {
  const options = parseArgs(argv);
  const pin = readPin();
  for (const triple of options.targets) {
    if (!options.check) {
      await stageTarget({ dest: options.dest, triple, pin, prefixes: options.prefixes, exec });
    }
    const manifest = checkStaged({ dest: options.dest, triple, pin });
    log(`FFmpeg payload ${triple}: ${Object.keys(manifest.runtime).join(", ")} verified (LGPL, ${manifest.source.sha256})`);
  }
  if (options.prune && existsSync(options.dest)) {
    for (const name of readdirSync(options.dest)) {
      if (name.startsWith(".") || options.targets.includes(name)) continue;
      rmSync(join(options.dest, name), { recursive: true, force: true });
      log(`pruned ${name} from ${options.dest}`);
    }
  }
  return options.targets;
}

const invokedDirectly = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) {
  main().catch((error) => {
    console.error(`stage-ffmpeg: ${error.message}`);
    process.exit(1);
  });
}
