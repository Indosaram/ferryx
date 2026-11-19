// Reusable QA artifact set: seal a build-only directory into artifact-manifest.json and stage the
// sealed bytes into a fresh artifact directory that the run owns exclusively.
//
// Layout of a build-only directory (produced by the build lane, one dir per variant):
//   <directory>/artifacts/{A,B,C,bump[,v1bump][,baseline]}
//   A, B, C, bump, v1bump: mac  <Product>.app + ferryx-cli
//                          linux ferryx + ferryx-cli
//                          windows ferryx.exe + ferryx-cli.exe + every *.dll of the build output
//   baseline: only ferryx / ferryx.exe (a daemon-only cargo build, no generation key)
//
// The manifest is the single source of truth for the consumer: it names every file with its sha256
// and size, so staging copies a deterministic projection of the variant directory (incidental build
// metadata such as the mac outer-entitlements.plist is never part of the executed package) and the
// bytes are re-read and re-hashed after the copy. The producer hashes what it sealed; the consumer
// only ever trusts the manifest it was pinned to by sha256, never a manifest-named path outside the
// destination, and never follows a link out of a bundle.
//
// API:
//   sealArtifactSet({ directory, identity, mac? }) -> { sha256, manifest }
//   stageArtifactSet({ directory, sha256, identity, destination, mac? }) -> { manifest, artifacts, keys }
// `sha256` at stage is the externally pinned digest of the sealed manifest bytes.
// `mac` (tests only) injects { verify(target) -> {status,output}, plistValue(file,key) -> {status,value} };
// production defaults to /usr/bin/codesign and /usr/bin/plutil.

import { spawnSync } from 'node:child_process';
import { constants as fsConstants } from 'node:fs';
import { createHash } from 'node:crypto';
import {
  chmod, lstat, mkdir, open, readFile, readdir, readlink, realpath, rename, rm, symlink, writeFile,
} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { HELPER_BUNDLE_ID, HELPER_BUNDLE_NAME } from '../../../macos-daemon-helper.mjs';
import { LIBRARIES as FFMPEG_LIBRARIES } from '../../../stage-ffmpeg.mjs';
import { GuardError, qaBundleIdentifier, qaProductName } from './guards.mjs';
import { generationKey } from './wire.mjs';
import { assertLinuxQaLibraries, isLinuxQaLibrary } from './linux-artifact-runtime.mjs';

export const SCHEMA = 'ferryx-qa-artifact-set/v1';
export const MANIFEST_NAME = 'artifact-manifest.json';

const REQUIRED_VARIANTS = Object.freeze(['A', 'B', 'C', 'bump']);
const PLATFORMS = Object.freeze(['linux', 'mac', 'windows']);
const IDENTITY_FIELDS = Object.freeze(['platform', 'arch', 'head', 'tree', 'runId', 'version', 'v1Head', 'baselineHead']);
const OID = /^[0-9a-f]{40}$/;
const HEX64 = /^[0-9a-f]{64}$/;
const RUN_ID = /^\d{8}t\d{6}z$/;
const VERSION = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/;
/** A Windows shared library carried beside the executables; names, never paths. */
const DLL = /^[A-Za-z0-9][A-Za-z0-9._-]*\.dll$/i;
const MAX_ENTRIES = 20_000;
const MAX_TREE_DEPTH = 64;
const MAX_LINK_HOPS = 8;
const CHUNK = 1 << 20;

function fail(code, detail) { throw new GuardError(code, detail); }

/**
 * A mac set carries bundle symlinks and needs the real signature checker, so without a test seam it
 * may only be produced and consumed on macOS. An injected seam (tests) may run anywhere.
 */
function assertNativeHost(platform, mac) {
  if (platform !== 'mac' || mac || process.platform === 'darwin') return;
  fail('ARTIFACT_SET_INPUT', `mac artifact sets must be sealed and staged on macOS, not ${process.platform}`);
}

const byPath = (a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
const exeName = (platform) => (platform === 'windows' ? 'ferryx.exe' : 'ferryx');
const cliName = (platform) => (platform === 'windows' ? 'ferryx-cli.exe' : 'ferryx-cli');
const modeOf = (st) => st.mode & 0o777;

/** True when child equals parent or lives beneath it (lexical, both already absolute/normalized). */
function contains(parent, child) {
  const rel = path.relative(parent, child);
  return rel === '' || (!rel.startsWith('..') && !path.isAbsolute(rel));
}

async function lstatOrNull(p) {
  try {
    return await lstat(p);
  } catch (error) {
    if (error?.code === 'ENOENT' || error?.code === 'ENOTDIR') return null;
    throw error;
  }
}

// ---- identity ----

/** The eight identity fields, validated. Unknown fields are ignored, missing/malformed ones refused. */
function normalizeIdentity(input, label) {
  if (!input || typeof input !== 'object' || Array.isArray(input)) fail('ARTIFACT_SET_IDENTITY', `${label} must be an object`);
  const { platform, arch, head, tree, runId, version, v1Head, baselineHead } = input;
  if (!PLATFORMS.includes(platform)) fail('ARTIFACT_SET_IDENTITY', `${label}.platform ${JSON.stringify(platform)} is not one of ${PLATFORMS.join('|')}`);
  if (typeof arch !== 'string' || !arch.trim()) fail('ARTIFACT_SET_IDENTITY', `${label}.arch must be a node arch string`);
  if (!OID.test(String(head ?? ''))) fail('ARTIFACT_SET_IDENTITY', `${label}.head must be a full 40-character git commit id`);
  if (!OID.test(String(tree ?? ''))) fail('ARTIFACT_SET_IDENTITY', `${label}.tree must be a full 40-character git tree id`);
  if (typeof runId !== 'string' || !RUN_ID.test(runId)) fail('ARTIFACT_SET_IDENTITY', `${label}.runId ${JSON.stringify(runId)} is not a harness run id`);
  if (typeof version !== 'string' || !VERSION.test(version)) fail('ARTIFACT_SET_IDENTITY', `${label}.version ${JSON.stringify(version)} is not a Cargo version`);
  for (const [key, value] of [['v1Head', v1Head], ['baselineHead', baselineHead]]) {
    if (value !== null && !OID.test(String(value))) fail('ARTIFACT_SET_IDENTITY', `${label}.${key} must be a full commit id or null`);
  }
  return { platform, arch, head, tree, runId, version, v1Head, baselineHead };
}

function sameIdentity(stored, expected) {
  return IDENTITY_FIELDS.every((field) => stored[field] === expected[field]);
}

/** A, B, C and bump always; v1bump/baseline exactly when the identity pins their revision. */
function expectedVariants(identity) {
  const names = [...REQUIRED_VARIANTS];
  if (identity.v1Head !== null) names.push('v1bump');
  if (identity.baselineHead !== null) names.push('baseline');
  return names;
}

// ---- hashing and walking ----

/** sha256 and size of one regular file, read in bounded chunks, stable-size checked. */
async function hashFile(abs) {
  const handle = await open(abs, fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW ?? 0));
  try {
    const st = await handle.stat();
    if (!st.isFile()) fail('ARTIFACT_SET_PAYLOAD', `not a regular file: ${abs}`);
    const hash = createHash('sha256');
    const buf = Buffer.allocUnsafe(CHUNK);
    let total = 0;
    for (;;) {
      const { bytesRead } = await handle.read(buf, 0, CHUNK, null);
      if (bytesRead === 0) break;
      hash.update(buf.subarray(0, bytesRead));
      total += bytesRead;
    }
    if (total !== st.size) fail('ARTIFACT_SET_PAYLOAD', `${abs} changed size while it was hashed`);
    return { sha256: hash.digest('hex'), size: total };
  } finally {
    await handle.close();
  }
}

/**
 * A symlink inside a build tree is only safe when it stays inside the bundle it belongs to and its
 * chain terminates in a real node; absolute targets, escaping targets and cycles are refused.
 */
async function assertInternalLink({ abs, link, rootAbs }) {
  if (path.isAbsolute(link) || link.startsWith('\\') || /^[A-Za-z]:/.test(link)) {
    fail('ARTIFACT_SET_PATH', `absolute symlink is refused: ${abs} -> ${link}`);
  }
  let cursor = abs;
  for (let hops = 0; ; hops += 1) {
    const st = await lstatOrNull(cursor);
    if (!st) fail('ARTIFACT_SET_PATH', `dangling symlink ${abs} -> ${link}`);
    if (!st.isSymbolicLink()) break;
    if (hops >= MAX_LINK_HOPS) fail('ARTIFACT_SET_PATH', `symlink chain longer than ${MAX_LINK_HOPS} at ${abs}`);
    const next = path.resolve(path.dirname(cursor), await readlink(cursor));
    if (!contains(rootAbs, next)) fail('ARTIFACT_SET_PATH', `symlink ${abs} -> ${link} escapes ${rootAbs}`);
    cursor = next;
  }
  if (!contains(await realpath(rootAbs), await realpath(cursor))) fail('ARTIFACT_SET_PATH', `symlink ${abs} -> ${link} escapes ${rootAbs}`);
}

/** One directory entry of a tree, appended in readdir order; refuses nonregular nodes. */
async function collectNode({ abs, rel, st, entries, allowLinks, linkRootAbs, depth }) {
  if (entries.length > MAX_ENTRIES) fail('ARTIFACT_SET_VARIANT', `more than ${MAX_ENTRIES} entries under ${rel}`);
  if (st.isSymbolicLink()) {
    if (!allowLinks) fail('ARTIFACT_SET_VARIANT', `symlink ${rel} is not allowed in this variant`);
    const link = await readlink(abs);
    await assertInternalLink({ abs, link, rootAbs: linkRootAbs });
    entries.push({ path: rel, type: 'symlink', link });
    return;
  }
  if (st.isDirectory()) {
    if (depth > MAX_TREE_DEPTH) fail('ARTIFACT_SET_VARIANT', `tree deeper than ${MAX_TREE_DEPTH} at ${rel}`);
    entries.push({ path: rel, type: 'directory', mode: modeOf(st) });
    for (const name of (await readdir(abs)).sort()) {
      const childAbs = path.join(abs, name);
      await collectNode({
        abs: childAbs, rel: `${rel}/${name}`, st: await lstat(childAbs), entries, allowLinks, linkRootAbs, depth: depth + 1,
      });
    }
    return;
  }
  if (!st.isFile()) fail('ARTIFACT_SET_VARIANT', `nonregular file is refused: ${rel}`);
  const digest = await hashFile(abs);
  entries.push({ path: rel, type: 'file', size: digest.size, mode: modeOf(st), sha256: digest.sha256 });
}

/** All entries of a tree rooted at `rootAbs` (the root itself excluded), paths relative, sorted. */
async function collectTreeEntries({ rootAbs, allowLinks, linkRootAbs }) {
  const entries = [];
  for (const name of (await readdir(rootAbs)).sort()) {
    const abs = path.join(rootAbs, name);
    await collectNode({ abs, rel: name, st: await lstat(abs), entries, allowLinks, linkRootAbs, depth: 1 });
  }
  entries.sort(byPath);
  return entries;
}

// ---- mac signature seam ----

/** Production tools are the OS binaries by absolute path; tests inject deterministic stand-ins. */
function macTools(injection) {
  const base = {
    verify(target) {
      const r = spawnSync('/usr/bin/codesign', ['--verify', '--deep', '--strict', '--verbose=2', target], { encoding: 'utf8' });
      return { status: r.error ? -1 : (r.status ?? -1), output: `${r.stdout ?? ''}${r.stderr ?? ''}${r.error ? ` ${r.error.message}` : ''}` };
    },
    plistValue(file, key) {
      const r = spawnSync('/usr/bin/plutil', ['-extract', key, 'raw', '-o', '-', file], { encoding: 'utf8' });
      return { status: r.error ? -1 : (r.status ?? -1), value: String(r.stdout ?? '') };
    },
  };
  const injected = injection ?? {};
  return { verify: injected.verify ?? base.verify, plistValue: injected.plistValue ?? base.plistValue };
}

function plistField(tools, file, key, label) {
  const result = tools.plistValue(file, key);
  const value = String(result?.value ?? '').trim();
  if (result?.status !== 0 || !value) fail('ARTIFACT_SET_SIGNATURE', `${label}: cannot read ${key} from ${file}`);
  return value;
}

/**
 * Verifies a QA .app and its nested daemon helper with the real signature check and the baked QA
 * bundle identifier. Returns the bundle's own executable names so the manifest never guesses them.
 */
async function verifyMacBundle({ appDir, identity, mac, label }) {
  const tools = macTools(mac);
  const info = path.join(appDir, 'Contents', 'Info.plist');
  const executable = plistField(tools, info, 'CFBundleExecutable', label);
  const bundleIdentifier = plistField(tools, info, 'CFBundleIdentifier', label);
  const expected = qaBundleIdentifier(identity.runId);
  if (bundleIdentifier !== expected) {
    fail('ARTIFACT_SET_SIGNATURE', `${label}: CFBundleIdentifier is ${JSON.stringify(bundleIdentifier)}, expected ${expected}`);
  }
  const daemonDir = path.join(appDir, 'Contents', 'Helpers', HELPER_BUNDLE_NAME);
  const daemonInfo = path.join(daemonDir, 'Contents', 'Info.plist');
  const daemonExecutable = plistField(tools, daemonInfo, 'CFBundleExecutable', `${label} daemon`);
  const daemonIdentifier = plistField(tools, daemonInfo, 'CFBundleIdentifier', `${label} daemon`);
  if (daemonIdentifier !== HELPER_BUNDLE_ID) {
    fail('ARTIFACT_SET_SIGNATURE', `${label}: nested daemon identifier is ${JSON.stringify(daemonIdentifier)}, expected ${HELPER_BUNDLE_ID}`);
  }
  for (const target of [appDir, daemonDir]) {
    const result = tools.verify(target);
    if (result?.status !== 0) {
      fail('ARTIFACT_SET_SIGNATURE', `codesign --verify --deep --strict refused ${target}: ${String(result?.output ?? '').trim()}`);
    }
  }
  return { executable, daemonExecutable };
}

// ---- seal ----

async function realDirectory(dir, label) {
  if (typeof dir !== 'string' || !dir) fail('ARTIFACT_SET_INPUT', `${label} is required`);
  if (!path.isAbsolute(dir)) fail('ARTIFACT_SET_INPUT', `${label} must be an absolute path: ${dir}`);
  const abs = path.normalize(dir);
  const st = await lstatOrNull(abs);
  if (!st) fail('ARTIFACT_SET_INPUT', `${label} does not exist: ${abs}`);
  if (st.isSymbolicLink() || !st.isDirectory()) fail('ARTIFACT_SET_INPUT', `${label} must be a real directory: ${abs}`);
  return realpath(abs);
}

async function readDirReal(dir, label) {
  const st = await lstatOrNull(dir);
  if (!st) fail('ARTIFACT_SET_VARIANT', `${label} is missing: ${dir}`);
  if (st.isSymbolicLink() || !st.isDirectory()) fail('ARTIFACT_SET_VARIANT', `${label} must be a real directory: ${dir}`);
  return (await readdir(dir)).sort();
}

/** The fixed entries of one variant: everything else in the variant directory is build metadata. */
function variantRoots({ names, name, identity }) {
  const { platform } = identity;
  if (platform === 'linux') {
    const roots = (name === 'baseline' ? ['ferryx'] : ['ferryx', 'ferryx-cli']).map((rel) => ({ rel, dir: false, allowLinks: false }));
    if (!['v1bump', 'baseline'].includes(name) || names.includes('ffmpeg')) roots.push({ rel: 'ffmpeg', dir: true, allowLinks: false });
    return roots;
  }
  if (name === 'baseline') return [{ rel: exeName(platform), dir: false, allowLinks: false }];
  if (platform === 'mac') {
    return [
      { rel: `${qaProductName(identity.runId)}.app`, dir: true, allowLinks: true },
      { rel: 'ferryx-cli', dir: false, allowLinks: false },
    ];
  }
  if (platform === 'windows') {
    const dlls = names.filter((entry) => DLL.test(entry));
    for (const lib of FFMPEG_LIBRARIES) {
      if (!dlls.some((dll) => new RegExp(`^${lib}-[0-9]+\\.dll$`, 'i').test(dll))) fail('ARTIFACT_SET_VARIANT', `${name}: missing Windows runtime ${lib}-<major>.dll`);
    }
    return [...['ferryx.exe', 'ferryx-cli.exe'], ...dlls].map((rel) => ({ rel, dir: false, allowLinks: false }));
  }
  return ['ferryx', 'ferryx-cli'].map((rel) => ({ rel, dir: false, allowLinks: false }));
}

function fileEntry(entries, rel, label) {
  const entry = entries.find((candidate) => candidate.path === rel);
  if (!entry || entry.type !== 'file') fail('ARTIFACT_SET_VARIANT', `${label}: ${rel} is not a sealed file`);
  return entry;
}

async function collectVariant({ dir, name, identity, mac }) {
  const { platform } = identity;
  const names = await readDirReal(dir, `artifacts/${name}`);
  if (name === 'baseline') {
    const expected = exeName(platform);
    const files = names.filter((entry) => platform !== 'linux' || entry !== 'ffmpeg');
    if (files.length !== 1 || files[0] !== expected) {
      fail('ARTIFACT_SET_VARIANT', `artifacts/${name} must hold only ${expected}, found ${JSON.stringify(names)}`);
    }
  }
  const appRel = `${qaProductName(identity.runId)}.app`;
  const roots = variantRoots({ names, name, identity });
  if (platform === 'linux' && roots.some((r) => r.rel === 'ffmpeg')) {
    assertLinuxQaLibraries(await readDirReal(path.join(dir, 'ffmpeg'), `artifacts/${name}/ffmpeg`));
  }
  for (const root of roots) {
    const st = await lstatOrNull(path.join(dir, root.rel));
    if (!st) fail('ARTIFACT_SET_VARIANT', `artifacts/${name}/${root.rel} is missing`);
    if (st.isSymbolicLink() || (root.dir ? !st.isDirectory() : !st.isFile())) {
      fail('ARTIFACT_SET_VARIANT', `artifacts/${name}/${root.rel} must be a real ${root.dir ? 'directory' : 'regular file'}`);
    }
  }
  // mac: the signature is checked before the bytes are hashed, so the seal covers a verified bundle.
  const macInfo = platform === 'mac' && name !== 'baseline'
    ? await verifyMacBundle({ appDir: path.join(dir, appRel), identity, mac, label: `artifacts/${name}/${appRel}` })
    : null;

  const entries = [];
  for (const root of roots) {
    const abs = path.join(dir, root.rel);
    await collectNode({
      abs, rel: root.rel, st: await lstat(abs), entries, allowLinks: root.allowLinks, linkRootAbs: root.dir ? abs : null, depth: 0,
    });
  }
  // Two roots can never name the same path; sorting makes the manifest order canonical.
  entries.sort(byPath);
  for (let i = 1; i < entries.length; i += 1) {
    if (entries[i - 1].path === entries[i].path) fail('ARTIFACT_SET_VARIANT', `duplicate entry ${entries[i].path}`);
  }

  let artifacts = null;
  let daemonPath;
  if (name === 'baseline') {
    daemonPath = exeName(platform);
  } else if (platform === 'mac') {
    artifacts = {
      appPath: appRel,
      guiExecutable: `${appRel}/Contents/MacOS/${macInfo.executable}`,
      cliExecutable: 'ferryx-cli',
      daemonSource: `${appRel}/Contents/Helpers/${HELPER_BUNDLE_NAME}/Contents/MacOS/${macInfo.daemonExecutable}`,
    };
    daemonPath = artifacts.daemonSource;
  } else {
    artifacts = { appPath: null, guiExecutable: exeName(platform), cliExecutable: cliName(platform), daemonSource: exeName(platform) };
    daemonPath = artifacts.daemonSource;
  }
  const daemon = fileEntry(entries, daemonPath, `artifacts/${name}`);
  return {
    name,
    platform,
    ...(platform === 'mac' && name !== 'baseline' ? { bundleIdentifier: qaBundleIdentifier(identity.runId) } : {}),
    generationKey: name === 'baseline' ? null : generationKey(identity.version, daemon.sha256),
    entries,
    artifacts,
    daemon: { path: daemon.path, sha256: daemon.sha256, size: daemon.size },
  };
}

function assertDistinctGenerationKeys(variants) {
  const seen = new Map();
  for (const [name, variant] of Object.entries(variants)) {
    if (variant.generationKey === null) continue;
    if (seen.has(variant.generationKey)) fail('ARTIFACT_SET_VARIANT', `duplicated generation keys: ${seen.get(variant.generationKey)} and ${name} both ${variant.generationKey}`);
    seen.set(variant.generationKey, name);
  }
}

function serializeManifest(manifest) {
  return `${JSON.stringify(manifest)}\n`;
}

async function writeAtomic(file, text) {
  const temp = `${file}.tmp-${process.pid}-${Date.now()}`;
  await writeFile(temp, text, { mode: 0o600 });
  await rename(temp, file);
}

/**
 * Seals the build-only directory: hashes the fixed expected entries of every variant, verifies mac
 * bundles, and writes artifact-manifest.json beside artifacts/. Returns the manifest digest the
 * consumer pins with --manifest-sha256.
 */
export async function sealArtifactSet({ directory, identity, mac } = {}) {
  const id = normalizeIdentity(identity, 'identity');
  assertNativeHost(id.platform, mac);
  const buildDir = await realDirectory(directory, 'directory');
  const artifactsDir = path.join(buildDir, 'artifacts');
  const names = await readDirReal(artifactsDir, 'artifacts');
  const expected = expectedVariants(id);
  for (const name of names) {
    if (!expected.includes(name)) fail('ARTIFACT_SET_VARIANT', `unexpected variant ${name}; expected ${expected.join(', ')}`);
  }
  for (const name of expected) {
    if (!names.includes(name)) fail('ARTIFACT_SET_VARIANT', `missing variant ${name} in ${artifactsDir}`);
  }
  const variants = {};
  for (const name of expected) {
    variants[name] = await collectVariant({ dir: path.join(artifactsDir, name), name, identity: id, mac });
  }
  assertDistinctGenerationKeys(variants);
  const manifest = { schema: SCHEMA, identity: id, variants };
  const text = serializeManifest(manifest);
  const sha256 = createHash('sha256').update(text).digest('hex');
  await writeAtomic(path.join(buildDir, MANIFEST_NAME), text);
  return { sha256, manifest };
}

// ---- manifest validation ----

function checkKeys(value, required, optional, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) fail('ARTIFACT_SET_MANIFEST', `${label} must be an object`);
  const allowed = new Set([...required, ...optional]);
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) fail('ARTIFACT_SET_MANIFEST', `${label} has an unexpected field ${key}`);
  }
  for (const key of required) {
    if (!Object.hasOwn(value, key)) fail('ARTIFACT_SET_MANIFEST', `${label} is missing ${key}`);
  }
}

/** Relative, canonical, posix-only manifest path: no absolute, no drive, no traversal, no separator alias. */
function canonicalRelative(value, label) {
  if (typeof value !== 'string' || !value) fail('ARTIFACT_SET_PATH', `${label} must be a non-empty relative path`);
  if (value.includes('\\') || value.includes('\0') || value.includes(':')) fail('ARTIFACT_SET_PATH', `${label} ${JSON.stringify(value)} is not a canonical posix path`);
  if (value.startsWith('/') || path.posix.normalize(value) !== value) fail('ARTIFACT_SET_PATH', `${label} ${JSON.stringify(value)} is not canonical`);
  for (const segment of value.split('/')) {
    if (!segment || segment === '.' || segment === '..') fail('ARTIFACT_SET_PATH', `${label} ${JSON.stringify(value)} escapes its root`);
  }
  return value;
}

/** Lexical containment of a signed-link target inside the app root, before anything is created. */
function assertLinkInside({ rel, link, root }) {
  if (typeof link !== 'string' || !link) fail('ARTIFACT_SET_PATH', `${rel}: symlink target is missing`);
  if (path.posix.isAbsolute(link) || link.includes('\\') || /^[A-Za-z]:/.test(link)) fail('ARTIFACT_SET_PATH', `${rel}: absolute symlink target ${JSON.stringify(link)}`);
  const floor = root.split('/').length;
  const stack = rel.slice(0, rel.lastIndexOf('/')).split('/');
  for (const part of link.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') {
      stack.pop();
      if (stack.length < floor) fail('ARTIFACT_SET_PATH', `${rel}: symlink target ${JSON.stringify(link)} escapes ${root}`);
      continue;
    }
    stack.push(part);
  }
}

function expectedEntryRoots({ platform, name, runId, entries }) {
  if (platform === 'linux') {
    const roots = name === 'baseline' ? ['ferryx'] : ['ferryx', 'ferryx-cli'];
    if (!['v1bump', 'baseline'].includes(name) || entries.some((e) => e.path === 'ffmpeg')) {
      const libs = entries.filter((e) => e.path.startsWith('ffmpeg/')).map((e) => e.path.slice(7));
      assertLinuxQaLibraries(libs);
      roots.push('ffmpeg');
    }
    return roots;
  }
  if (name === 'baseline') return [exeName(platform)];
  if (platform === 'mac') return [`${qaProductName(runId)}.app`, 'ferryx-cli'];
  if (platform === 'windows') {
    const roots = new Set(['ferryx.exe', 'ferryx-cli.exe']);
    for (const entry of entries) {
      if (entry.path.includes('/')) continue;
      if (!DLL.test(entry.path)) continue;
      if (entry.type !== 'file') fail('ARTIFACT_SET_MANIFEST', `${entry.path} must be a DLL file`);
      roots.add(entry.path);
    }
    return [...roots];
  }
  return ['ferryx', 'ferryx-cli'];
}

function validateEntries(entries, { platform, name, runId }) {
  const top = (rel) => rel.split('/')[0];
  if (!Array.isArray(entries) || entries.length === 0) fail('ARTIFACT_SET_MANIFEST', `variants.${name}.entries must be a non-empty array`);
  if (entries.length > MAX_ENTRIES) fail('ARTIFACT_SET_MANIFEST', `variants.${name} has more than ${MAX_ENTRIES} entries`);
  const seen = new Map();
  let previous = null;
  for (const entry of entries) {
    checkKeys(entry, ['path', 'type'], ['size', 'mode', 'sha256', 'link'], `variants.${name}.entries[]`);
    const rel = canonicalRelative(entry.path, `variants.${name}.entries[].path`);
    if (previous !== null && previous >= rel) fail('ARTIFACT_SET_MANIFEST', `variants.${name} entries are not sorted and unique at ${rel}`);
    previous = rel;
    if (entry.type === 'directory') {
      checkKeys(entry, ['path', 'type', 'mode'], [], `${rel}`);
      if (!Number.isInteger(entry.mode) || entry.mode < 0 || entry.mode > 0o777) fail('ARTIFACT_SET_MANIFEST', `${rel} has an invalid mode`);
    } else if (entry.type === 'file') {
      checkKeys(entry, ['path', 'type', 'size', 'mode', 'sha256'], [], `${rel}`);
      if (!HEX64.test(String(entry.sha256))) fail('ARTIFACT_SET_MANIFEST', `${rel} has an invalid sha256`);
      if (!Number.isInteger(entry.size) || entry.size < 0) fail('ARTIFACT_SET_MANIFEST', `${rel} has an invalid size`);
      if (!Number.isInteger(entry.mode) || entry.mode < 0 || entry.mode > 0o777) fail('ARTIFACT_SET_MANIFEST', `${rel} has an invalid mode`);
    } else if (entry.type === 'symlink') {
      checkKeys(entry, ['path', 'type', 'link'], [], `${rel}`);
    } else {
      fail('ARTIFACT_SET_MANIFEST', `${rel} has unsupported type ${JSON.stringify(entry.type)}`);
    }
    const linuxRuntimeDir = platform === 'linux' && rel === 'ffmpeg' && entry.type === 'directory';
    if (entry.type !== 'symlink' && platform !== 'mac' && entry.type !== 'file' && !linuxRuntimeDir) {
      fail('ARTIFACT_SET_MANIFEST', `${rel}: only regular files are sealed on ${platform}`);
    }
    seen.set(rel, entry);
  }
  const roots = expectedEntryRoots({ platform, name, runId, entries });
  for (const rel of seen.keys()) {
    if (!roots.includes(top(rel))) fail('ARTIFACT_SET_MANIFEST', `variants.${name} names ${rel}, outside the expected entries ${JSON.stringify(roots)}`);
  }
  for (const root of roots) {
    if (!seen.has(root)) fail('ARTIFACT_SET_MANIFEST', `variants.${name} is missing ${root}`);
  }
  for (const [rel, entry] of seen) {
    const parent = rel.includes('/') ? rel.slice(0, rel.lastIndexOf('/')) : null;
    if (parent !== null) {
      const parentEntry = seen.get(parent);
      if (!parentEntry || parentEntry.type !== 'directory') fail('ARTIFACT_SET_MANIFEST', `${rel} has no directory entry ${parent}`);
    }
    if (entry.type === 'symlink') {
      if (platform !== 'mac' || top(rel) !== `${qaProductName(runId)}.app`) fail('ARTIFACT_SET_PATH', `${rel}: symlinks are only accepted inside the mac app bundle`);
      assertLinkInside({ rel, link: entry.link, root: `${qaProductName(runId)}.app` });
    }
  }
  if (platform === 'linux' && entries.some((e) => e.path.startsWith('ffmpeg/') && (!isLinuxQaLibrary(e.path.slice(7)) || e.type !== 'file'))) {
    fail('ARTIFACT_SET_MANIFEST', `variants.${name}: invalid Linux runtime entry`);
  }
  if (name === 'baseline' && platform !== 'linux' && (entries.length !== 1 || entries[0].type !== 'file')) {
    fail('ARTIFACT_SET_MANIFEST', `artifacts/baseline must seal exactly one file`);
  }
  return entries.map((entry) => ({ ...entry })).sort(byPath);
}

function validateDaemon(raw, byPathMap, name) {
  checkKeys(raw, ['path', 'sha256', 'size'], [], `variants.${name}.daemon`);
  const rel = canonicalRelative(raw.path, `variants.${name}.daemon.path`);
  const entry = byPathMap.get(rel);
  if (!entry || entry.type !== 'file') fail('ARTIFACT_SET_MANIFEST', `variants.${name}.daemon names no sealed file`);
  if (!HEX64.test(String(raw.sha256)) || raw.sha256 !== entry.sha256) fail('ARTIFACT_SET_MANIFEST', `variants.${name}.daemon sha256 is not the sealed file digest`);
  if (raw.size !== entry.size) fail('ARTIFACT_SET_MANIFEST', `variants.${name}.daemon size is not the sealed file size`);
  return { path: rel, sha256: entry.sha256, size: entry.size };
}

function validateArtifacts(raw, { name, platform, runId, byPathMap }) {
  checkKeys(raw, ['appPath', 'guiExecutable', 'cliExecutable', 'daemonSource'], [], `variants.${name}.artifacts`);
  const file = (value, label) => {
    const rel = canonicalRelative(value, `variants.${name}.artifacts.${label}`);
    const entry = byPathMap.get(rel);
    if (!entry || entry.type !== 'file') fail('ARTIFACT_SET_MANIFEST', `variants.${name}.artifacts.${label} names no sealed file`);
    return rel;
  };
  const guiExecutable = file(raw.guiExecutable, 'guiExecutable');
  const cliExecutable = file(raw.cliExecutable, 'cliExecutable');
  const daemonSource = file(raw.daemonSource, 'daemonSource');
  if (platform === 'mac') {
    const appPath = `${qaProductName(runId)}.app`;
    if (raw.appPath !== appPath) fail('ARTIFACT_SET_MANIFEST', `variants.${name}.artifacts.appPath must be ${appPath}`);
    const appEntry = byPathMap.get(appPath);
    if (!appEntry || appEntry.type !== 'directory') fail('ARTIFACT_SET_MANIFEST', `variants.${name} has no app directory entry`);
    if (!guiExecutable.startsWith(`${appPath}/Contents/MacOS/`)) fail('ARTIFACT_SET_MANIFEST', `variants.${name}.guiExecutable is outside the app`);
    if (!daemonSource.startsWith(`${appPath}/Contents/Helpers/${HELPER_BUNDLE_NAME}/`)) fail('ARTIFACT_SET_MANIFEST', `variants.${name}.daemonSource is outside the nested daemon`);
    if (cliExecutable !== 'ferryx-cli') fail('ARTIFACT_SET_MANIFEST', `variants.${name}.cliExecutable must be ferryx-cli`);
    return { appPath, guiExecutable, cliExecutable, daemonSource };
  }
  if (raw.appPath !== null) fail('ARTIFACT_SET_MANIFEST', `variants.${name}.artifacts.appPath must be null on ${platform}`);
  if (guiExecutable !== exeName(platform)) fail('ARTIFACT_SET_MANIFEST', `variants.${name}.guiExecutable must be ${exeName(platform)}`);
  if (cliExecutable !== cliName(platform)) fail('ARTIFACT_SET_MANIFEST', `variants.${name}.cliExecutable must be ${cliName(platform)}`);
  if (daemonSource !== exeName(platform)) fail('ARTIFACT_SET_MANIFEST', `variants.${name}.daemonSource must be ${exeName(platform)}`);
  return { appPath: null, guiExecutable, cliExecutable, daemonSource };
}

function validateVariant(raw, { name, identity }) {
  const isMacApp = identity.platform === 'mac' && name !== 'baseline';
  checkKeys(raw, ['name', 'platform', 'generationKey', 'entries', 'artifacts', 'daemon'], isMacApp ? ['bundleIdentifier'] : [], `variants.${name}`);
  if (raw.name !== name) fail('ARTIFACT_SET_MANIFEST', `variants.${name}.name is ${JSON.stringify(raw.name)}`);
  if (raw.platform !== identity.platform) fail('ARTIFACT_SET_MANIFEST', `variants.${name}.platform is ${JSON.stringify(raw.platform)}`);
  if (isMacApp) {
    const expected = qaBundleIdentifier(identity.runId);
    if (raw.bundleIdentifier !== expected) fail('ARTIFACT_SET_SIGNATURE', `variants.${name}.bundleIdentifier is not the baked QA identifier ${expected}`);
  }
  const entries = validateEntries(raw.entries, { platform: identity.platform, name, runId: identity.runId });
  const byPathMap = new Map(entries.map((entry) => [entry.path, entry]));
  const daemon = validateDaemon(raw.daemon, byPathMap, name);
  const artifacts = name === 'baseline'
    ? null
    : validateArtifacts(raw.artifacts, { name, platform: identity.platform, runId: identity.runId, byPathMap });
  if (name === 'baseline' && raw.artifacts !== null) fail('ARTIFACT_SET_MANIFEST', `variants.baseline has no artifact descriptor`);
  const expectedKey = name === 'baseline' ? null : generationKey(identity.version, daemon.sha256);
  if (raw.generationKey !== expectedKey) fail('ARTIFACT_SET_VARIANT', `variants.${name}.generationKey ${JSON.stringify(raw.generationKey)} is not ${JSON.stringify(expectedKey)}`);
  if (daemon.path !== (artifacts ? artifacts.daemonSource : exeName(identity.platform))) {
    fail('ARTIFACT_SET_MANIFEST', `variants.${name}.daemon is not the artifact's daemon source`);
  }
  return {
    name,
    platform: identity.platform,
    ...(isMacApp ? { bundleIdentifier: raw.bundleIdentifier } : {}),
    generationKey: expectedKey,
    entries,
    artifacts,
    daemon,
  };
}

function validateManifest(parsed, identity) {
  checkKeys(parsed, ['schema', 'identity', 'variants'], [], 'manifest');
  if (parsed.schema !== SCHEMA) fail('ARTIFACT_SET_MANIFEST', `unsupported schema ${JSON.stringify(parsed.schema)}`);
  checkKeys(parsed.identity, IDENTITY_FIELDS, [], 'manifest.identity');
  const stored = normalizeIdentity(parsed.identity, 'manifest.identity');
  if (!sameIdentity(stored, identity)) {
    const field = IDENTITY_FIELDS.find((candidate) => stored[candidate] !== identity[candidate]);
    fail('ARTIFACT_SET_IDENTITY', `manifest ${field} ${JSON.stringify(stored[field])} is not this run's ${JSON.stringify(identity[field])}`);
  }
  const names = expectedVariants(identity);
  checkKeys(parsed.variants, names, [], 'manifest.variants');
  if (Object.keys(parsed.variants).length !== names.length) fail('ARTIFACT_SET_VARIANT', `manifest variants are not exactly ${JSON.stringify(names)}`);
  const variants = {};
  for (const name of names) variants[name] = validateVariant(parsed.variants[name], { name, identity });
  assertDistinctGenerationKeys(variants);
  return { schema: SCHEMA, identity, variants };
}

// ---- stage ----

/**
 * Refuses destinations that are not a fresh child of a run-owned root. The path is a real local
 * path, so its flavour and the refused system/user roots follow this OS, not the artifact platform.
 */
function assertSafeDestination(abs) {
  const windows = process.platform === 'win32';
  const mod = windows ? path.win32 : path.posix;
  const sep = windows ? '\\' : '/';
  const root = mod.parse(abs).root;
  if (!root || abs === root) fail('ARTIFACT_SET_DESTINATION', `${abs} is a filesystem root`);
  const parts = abs.slice(root.length).split(sep).filter(Boolean);
  if (parts.length < 2) fail('ARTIFACT_SET_DESTINATION', `${abs} is too close to the filesystem root`);
  const lower = windows ? abs.toLowerCase() : abs;
  const blocked = windows
    ? ['C:\\Windows', 'C:\\Program Files', 'C:\\Program Files (x86)', ...(process.env.LOCALAPPDATA ? [`${process.env.LOCALAPPDATA}\\Ferryx`] : [])]
    : ['/Applications', '/Library', '/System', '/bin', '/sbin', '/usr', '/etc', '/private/etc'];
  for (const dir of blocked) {
    const target = windows ? dir.toLowerCase() : dir;
    if (lower === target || lower.startsWith(`${target}${sep}`)) fail('ARTIFACT_SET_DESTINATION', `${abs} is inside ${dir}`);
  }
  for (const name of ['Library', 'Applications', 'Desktop', 'Documents', 'Downloads', 'AppData']) {
    const dir = mod.join(os.homedir(), name);
    if (contains(dir, abs)) fail('ARTIFACT_SET_DESTINATION', `${abs} is inside ${dir}`);
  }
}

async function prepareDestination({ destination, source }) {
  if (typeof destination !== 'string' || !destination) fail('ARTIFACT_SET_DESTINATION', 'destination is required');
  if (!path.isAbsolute(destination)) fail('ARTIFACT_SET_DESTINATION', `destination must be an absolute path: ${destination}`);
  const abs = path.normalize(destination);
  assertSafeDestination(abs);
  if (await lstatOrNull(abs)) fail('ARTIFACT_SET_DESTINATION', `${abs} already exists; staging needs a fresh artifact directory`);
  const parent = path.dirname(abs);
  const parentStat = await lstatOrNull(parent);
  if (!parentStat) fail('ARTIFACT_SET_DESTINATION', `parent of ${abs} does not exist`);
  // A symlinked parent (/tmp on macOS) is the caller's own run-owned root: resolve it to decide
  // overlap, but keep the caller's spelling for everything the run then uses.
  const resolvedParent = await realpath(parent).catch((error) => fail('ARTIFACT_SET_DESTINATION', `${parent}: ${error?.code ?? error?.message}`));
  if (!(await lstatOrNull(resolvedParent))?.isDirectory()) fail('ARTIFACT_SET_DESTINATION', `${resolvedParent} must be a directory`);
  const resolved = path.join(resolvedParent, path.basename(abs));
  if (contains(source, resolved) || contains(resolved, source)) fail('ARTIFACT_SET_DESTINATION', `${abs} overlaps the sealed build directory ${source}`);
  try {
    await mkdir(abs, { mode: 0o700 });
  } catch (error) {
    fail('ARTIFACT_SET_DESTINATION', `cannot create ${abs}: ${error?.code ?? error?.message}`);
  }
  return abs;
}

/** Copy one sealed file: never follows a link at the source, refuses a size/digest change mid-copy. */
async function copyFileHashed({ src, dst, entry }) {
  const input = await open(src, fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW ?? 0)).catch((error) => {
    fail('ARTIFACT_SET_PAYLOAD', `cannot read ${src}: ${error?.code ?? error?.message}`);
  });
  try {
    const st = await input.stat();
    if (!st.isFile()) fail('ARTIFACT_SET_PAYLOAD', `${src} is no longer a regular file`);
    if (st.size !== entry.size) fail('ARTIFACT_SET_PAYLOAD', `${src} is ${st.size} bytes, the manifest sealed ${entry.size}`);
    const hash = createHash('sha256');
    let written = 0;
    const out = await open(dst, 'wx', entry.mode || 0o600).catch((error) => {
      fail('ARTIFACT_SET_DESTINATION', `cannot create ${dst}: ${error?.code ?? error?.message}`);
    });
    try {
      const buf = Buffer.allocUnsafe(CHUNK);
      for (;;) {
        const { bytesRead } = await input.read(buf, 0, CHUNK, null);
        if (bytesRead === 0) break;
        hash.update(buf.subarray(0, bytesRead));
        await out.writeFile(buf.subarray(0, bytesRead));
        written += bytesRead;
      }
    } finally {
      await out.close();
    }
    if (written !== entry.size) fail('ARTIFACT_SET_PAYLOAD', `${src} changed size while it was copied`);
    const actual = hash.digest('hex');
    if (actual !== entry.sha256) fail('ARTIFACT_SET_PAYLOAD', `${src} hashes ${actual}, the manifest sealed ${entry.sha256}`);
    if (process.platform !== 'win32' && entry.mode) await chmod(dst, entry.mode);
  } finally {
    await input.close();
  }
}

async function copyEntries({ sourceRoot, destRoot, entries, label }) {
  for (const entry of entries) {
    const dst = path.join(destRoot, ...entry.path.split('/'));
    if (!contains(destRoot, dst) || dst === destRoot) fail('ARTIFACT_SET_PATH', `${label}: ${entry.path} escapes ${destRoot}`);
    if (entry.type === 'directory') {
      try {
        await mkdir(dst, { mode: 0o700 });
      } catch (error) {
        fail('ARTIFACT_SET_DESTINATION', `${label}: cannot create ${dst}: ${error?.code ?? error?.message}`);
      }
      if (process.platform !== 'win32' && entry.mode) await chmod(dst, entry.mode | 0o700);
      continue;
    }
    if (entry.type === 'symlink') {
      try {
        await symlink(entry.link, dst);
      } catch (error) {
        fail('ARTIFACT_SET_DESTINATION', `${label}: cannot link ${dst}: ${error?.code ?? error?.message}`);
      }
      continue;
    }
    const src = path.join(sourceRoot, ...entry.path.split('/'));
    // The destination portion was created by this call; a symlinked parent can only come from a
    // manifest that tried to route a write through a link, so refuse instead of following it.
    const parent = path.dirname(dst);
    const parentStat = await lstat(parent).catch(() => null);
    if (!parentStat || parentStat.isSymbolicLink() || !parentStat.isDirectory()) {
      fail('ARTIFACT_SET_PATH', `${label}: refusing to write ${entry.path} below a non-directory`);
    }
    await copyFileHashed({ src, dst, entry });
  }
}

/** Re-reads and hashes the staged bytes; the returned set must equal the sealed set exactly. */
async function verifyTree({ variantRoot, entries, platform, label }) {
  const actual = await collectTreeEntries({
    rootAbs: variantRoot, allowLinks: platform === 'mac', linkRootAbs: variantRoot,
  });
  if (actual.length !== entries.length) {
    fail('ARTIFACT_SET_PAYLOAD', `${label}: ${actual.length} entries staged, ${entries.length} sealed`);
  }
  for (let i = 0; i < entries.length; i += 1) {
    const want = entries[i];
    const got = actual[i];
    if (want.path !== got.path) fail('ARTIFACT_SET_PAYLOAD', `${label}: staged ${got.path} where ${want.path} was sealed`);
    if (want.type !== got.type) fail('ARTIFACT_SET_PAYLOAD', `${label}/${want.path}: staged ${got.type}, sealed ${want.type}`);
    if (want.type === 'file' && (want.sha256 !== got.sha256 || want.size !== got.size)) {
      fail('ARTIFACT_SET_PAYLOAD', `${label}/${want.path}: staged bytes do not match the sealed digest`);
    }
    if (want.type === 'symlink' && want.link !== got.link) fail('ARTIFACT_SET_PAYLOAD', `${label}/${want.path}: staged link target differs`);
  }
}

/**
 * Stages the pinned manifest into a fresh destination directory:
 *   <destination>/artifact-manifest.json        the exact sealed bytes (hash still matches the pin)
 *   <destination>/artifacts/<variant>/...       the projection the manifest names
 * Returns run.mjs-shaped descriptors whose paths point into the destination, and generation keys.
 */
export async function stageArtifactSet({ directory, sha256, identity, destination, mac } = {}) {
  const id = normalizeIdentity(identity, 'identity');
  if (!HEX64.test(String(sha256 ?? ''))) fail('ARTIFACT_SET_MANIFEST', 'the pinned manifest sha256 must be 64 lowercase hex characters');
  const buildDir = await realDirectory(directory, 'directory');
  const manifestPath = path.join(buildDir, MANIFEST_NAME);
  const manifestStat = await lstatOrNull(manifestPath);
  if (!manifestStat) fail('ARTIFACT_SET_MANIFEST', `${manifestPath} is missing`);
  if (manifestStat.isSymbolicLink() || !manifestStat.isFile()) fail('ARTIFACT_SET_MANIFEST', `${manifestPath} must be a regular file`);
  const bytes = await readFile(manifestPath);
  const actualPin = createHash('sha256').update(bytes).digest('hex');
  if (actualPin !== sha256) fail('ARTIFACT_SET_MANIFEST', `manifest sha256 ${actualPin} does not match the pinned ${sha256}`);
  const text = bytes.toString('utf8');
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    fail('ARTIFACT_SET_MANIFEST', `${manifestPath} is not JSON: ${error?.message}`);
  }
  if (serializeManifest(parsed) !== text) fail('ARTIFACT_SET_MANIFEST', `${manifestPath} is not in the canonical sealed format`);
  const manifest = validateManifest(parsed, id);
  assertNativeHost(id.platform, mac);
  const artifactsDir = path.join(buildDir, 'artifacts');
  const variantNames = await readDirReal(artifactsDir, 'artifacts');
  if (JSON.stringify(variantNames) !== JSON.stringify(Object.keys(manifest.variants).sort())) fail('ARTIFACT_SET_VARIANT', 'source variant set changed since build');
  const destRoot = await prepareDestination({ destination, source: buildDir });
  try {
    await mkdir(path.join(destRoot, 'artifacts'), { mode: 0o700 });
    for (const [name, variant] of Object.entries(manifest.variants)) {
      const variantRoot = path.join(destRoot, 'artifacts', name);
      const sourceRoot = path.join(buildDir, 'artifacts', name);
      const sourceVariant = await collectVariant({ dir: sourceRoot, name, identity: id, mac });
      if (JSON.stringify(sourceVariant) !== JSON.stringify(variant)) fail('ARTIFACT_SET_PAYLOAD', `${name}: source payload no longer matches its sealed manifest`);
      await mkdir(variantRoot, { mode: 0o700 });
      await copyEntries({ sourceRoot, destRoot: variantRoot, entries: variant.entries, label: `artifacts/${name}` });
      await verifyTree({ variantRoot, entries: variant.entries, platform: id.platform, label: `artifacts/${name}` });
    }
    if (id.platform === 'mac') {
      for (const [name, variant] of Object.entries(manifest.variants)) {
        if (name === 'baseline') continue;
        const appDir = path.join(destRoot, 'artifacts', name, ...variant.artifacts.appPath.split('/'));
        await verifyMacBundle({ appDir, identity: id, mac, label: `staged artifacts/${name}/${variant.artifacts.appPath}` });
      }
    }
    await writeFile(path.join(destRoot, MANIFEST_NAME), text, { mode: 0o600 });
  } catch (error) {
    // The destination was created by this call under an exclusively owned root, so removing the
    // partial copy cannot discard anyone else's bytes and keeps a failed stage from being consumed.
    await rm(destRoot, { recursive: true, force: true });
    throw error;
  }

  const artifacts = {};
  const keys = {};
  for (const [name, variant] of Object.entries(manifest.variants)) {
    const base = path.join(destRoot, 'artifacts', name);
    if (name === 'baseline') {
      artifacts[name] = { name, daemonExecutable: path.join(base, ...variant.daemon.path.split('/')) };
      continue;
    }
    artifacts[name] = {
      name,
      appPath: variant.artifacts.appPath === null ? null : path.join(base, ...variant.artifacts.appPath.split('/')),
      guiExecutable: path.join(base, ...variant.artifacts.guiExecutable.split('/')),
      cliExecutable: path.join(base, ...variant.artifacts.cliExecutable.split('/')),
      daemonSource: path.join(base, ...variant.artifacts.daemonSource.split('/')),
      sha256: variant.daemon.sha256,
    };
    keys[name] = variant.generationKey;
  }
  return { manifest, artifacts, keys };
}
