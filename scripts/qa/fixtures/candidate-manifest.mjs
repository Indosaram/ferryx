#!/usr/bin/env bun
/**
 * Canonical candidate build manifest generator for Task 9 consistent recovery verification.
 * 
 * Strict contract:
 * - Reads actual compiled binary and UI dist artifacts.
 * - Computes real SHA-256 digests matching native-remote-agent.mjs hashTree logic.
 * - Strictly refuses execution if binary or UI distribution is absent.
 * - Never emits placeholders, invented stamps, or speculative manifest JSON before real build.
 */
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
export const CANONICAL_WORKTREE = resolve(__dirname, '../../..');
export const DEFAULT_BINARY = '/tmp/ferryx-recovery-final-target/debug/ferryx';
export const DEFAULT_UI_DIST = join(CANONICAL_WORKTREE, 'ui/dist');

export function validateHex(value, label, exactLength) {
  if (!value || typeof value !== 'string') {
    throw new Error(`Explicit --${label} is required from sole verifier retained patch receipt (no stale defaults permitted)`);
  }
  const clean = value.trim();
  if (!/^[0-9a-fA-F]+$/.test(clean) || clean.length !== exactLength) {
    throw new Error(`Invalid --${label} '${value}': must be exact ${exactLength}-char hex digest`);
  }
  return clean.toLowerCase();
}

export function hashFile(filePath) {
  if (!existsSync(filePath)) {
    throw new Error(`Candidate artifact absent: ${filePath}`);
  }
  return createHash('sha256').update(readFileSync(filePath)).digest('hex');
}

export function hashTree(dirPath) {
  if (!existsSync(dirPath)) {
    throw new Error(`Candidate artifact tree absent: ${dirPath}`);
  }
  const hash = createHash('sha256');
  function visit(directory) {
    for (const name of readdirSync(directory).sort()) {
      const file = join(directory, name);
      const info = statSync(file);
      if (info.isDirectory()) {
        visit(file);
      } else {
        hash.update(relative(dirPath, file).split('\\').join('/'));
        hash.update('\0');
        hash.update(readFileSync(file));
        hash.update('\0');
      }
    }
  }
  visit(dirPath);
  return hash.digest('hex');
}

export function parseCliArgs(argv) {
  const options = {
    binary: DEFAULT_BINARY,
    uiDist: DEFAULT_UI_DIST,
    baseRevision: null,
    patchHash: null,
  };

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--binary') options.binary = resolve(argv[++i]);
    else if (arg === '--ui-dist') options.uiDist = resolve(argv[++i]);
    else if (arg === '--base-revision') options.baseRevision = argv[++i];
    else if (arg === '--patch-hash') options.patchHash = argv[++i];
    else if (arg === '--dry-run') options.write = false;
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: bun scripts/qa/fixtures/candidate-manifest.mjs --base-revision <rev> --patch-hash <hash> [options]');
      console.log('Required from sole verifier retained patch receipt:');
      console.log('  --base-revision <hash>  Current base git commit revision (exact 40 hex chars)');
      console.log('  --patch-hash <hash>     Current retained candidate patch SHA-256 (exact 64 hex chars)');
      console.log('Options:');
      console.log('  --binary <path>         Candidate binary path (default: ' + DEFAULT_BINARY + ')');
      console.log('  --ui-dist <path>        Candidate UI dist directory (default: ' + DEFAULT_UI_DIST + ')');
      console.log('Output is written exclusively to stdout for apply_patch creation mandate.');
      process.exit(0);
    }
  }
  return options;
}

export function generateCandidateManifest(options = {}) {
  const binary = resolve(options.binary ?? DEFAULT_BINARY);
  const uiDist = resolve(options.uiDist ?? DEFAULT_UI_DIST);
  const baseRevision = validateHex(options.baseRevision, 'base-revision', 40);
  const patchHash = validateHex(options.patchHash, 'patch-hash', 64);

  // 1. Strict artifact existence guards - Refuse missing artifacts
  if (!existsSync(binary)) {
    throw new Error(`Candidate binary absent at '${binary}'; build required before manifest generation (CARGO_TARGET_DIR=/tmp/ferryx-recovery-final-target cargo build)`);
  }
  if (!statSync(binary).isFile()) {
    throw new Error(`Candidate binary path '${binary}' is not a regular file`);
  }

  if (!existsSync(uiDist)) {
    throw new Error(`Candidate UI dist absent at '${uiDist}'; UI build required before manifest generation (bun run --cwd ui build)`);
  }
  const indexHtml = join(uiDist, 'index.html');
  if (!existsSync(indexHtml)) {
    throw new Error(`Candidate UI dist at '${uiDist}' lacks index.html; incomplete UI build`);
  }

  // 2. Compute authentic cryptographic hashes
  const binarySha256 = hashFile(binary);
  const uiSha256 = hashTree(uiDist);

  // 3. Reject placeholders or degenerate hashes
  const isPlaceholder = (h) => !h || typeof h !== 'string' || h.startsWith('unverified') || h.length < 16;
  if (isPlaceholder(binarySha256) || isPlaceholder(uiSha256)) {
    throw new Error(`Computed artifact hash is invalid or placeholder: binary='${binarySha256}', ui='${uiSha256}'`);
  }

  const stamp = `${baseRevision}+${patchHash}`;

  const manifest = {
    stamp,
    baseRevision,
    patchHash,
    sourcePatchSha256: patchHash,
    binarySha256,
    uiSha256,
    builtAt: new Date().toISOString(),
  };

  return manifest;
}

// CLI entrypoint
const isMain = process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1]);
if (isMain) {
  try {
    const opts = parseCliArgs(process.argv.slice(2));
    const manifest = generateCandidateManifest(opts);
    process.stdout.write(JSON.stringify(manifest, null, 2) + '\n');
  } catch (err) {
    console.error(`MANIFEST_GENERATION_REFUSED: ${err.message}`);
    process.exit(1);
  }
}

