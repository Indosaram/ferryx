// On-disk half of the F3 verdict. contract.mjs judges the receipt; this file refuses to let a
// receipt vouch for bytes it does not have: every artifact path a step names is read back from
// the evidence dir and its sha256 and byte count recomputed, manifest.json must list exactly the
// files on disk with the same hashes, and the control host's recorded script hashes must equal
// the scripts that are validating now. Used by run.mjs at the end of a run and by --validate.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { hostname } from 'node:os';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { LANES, validateRun } from './contract.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const CHECKOUT = resolve(HERE, '..', '..', '..');
const sha = (bytes) => createHash('sha256').update(bytes).digest('hex');
const git = (...args) => execFileSync('git', ['-C', CHECKOUT, ...args], { encoding: 'utf8' }).trim();

/** The scripts that produce and judge F3 evidence, with their hashes. */
export function scriptHashes() {
  const files = [...readdirSync(HERE).filter((name) => name.endsWith('.mjs')).map((name) => join(HERE, name)),
    join(HERE, '..', 'herdr-reference-device.mjs'), join(HERE, '..', 'fixtures', 'native-configured-adapters.mjs'), join(HERE, '..', 'fixtures', 'native-remote-agent.mjs')];
  return files.sort().map((path) => ({ path: relative(CHECKOUT, path).replaceAll('\\', '/'), sha256: sha(readFileSync(path)) }));
}

/** Control host provenance: who ran the run, from which checkout, with which scripts. */
export function controlIdentity() {
  const status = git('status', '--porcelain', '--untracked-files=no');
  return { hostname: hostname(), checkout: CHECKOUT, head: git('rev-parse', 'HEAD'), clean: status === '', status, scripts: scriptHashes() };
}

/** Every file under the evidence dir except manifest.json, with bytes and sha256. */
export function manifestFiles(dir) {
  const files = [];
  const walk = (at) => {
    for (const name of readdirSync(at).sort()) {
      const path = join(at, name);
      const rel = relative(dir, path).replaceAll('\\', '/');
      if (statSync(path).isDirectory()) walk(path);
      else if (rel !== 'manifest.json') {
        const bytes = readFileSync(path);
        files.push({ path: rel, bytes: bytes.length, sha256: sha(bytes) });
      }
    }
  };
  walk(dir);
  return files;
}

function artifactErrors(dir, run) {
  const errors = [];
  for (const name of LANES) {
    for (const step of run?.lanes?.[name]?.steps ?? []) {
      for (const artifact of step.artifacts ?? []) {
        const where = `${name} ${step.scenarioId} ${artifact.kind} ${artifact.path}`;
        if (typeof artifact.path !== 'string' || isAbsolute(artifact.path) || artifact.path.split(/[\\/]/).includes('..')) { errors.push(`${where}: path escapes the evidence dir`); continue; }
        const path = join(dir, artifact.path);
        if (!existsSync(path)) { errors.push(`${where}: file missing`); continue; }
        const bytes = readFileSync(path);
        if (bytes.length !== artifact.bytes || sha(bytes) !== artifact.sha256) errors.push(`${where}: bytes/sha256 differ from the receipt`);
      }
    }
  }
  return errors;
}

function manifestErrors(dir) {
  const path = join(dir, 'manifest.json');
  if (!existsSync(path)) return ['manifest.json missing'];
  const listed = new Map((JSON.parse(readFileSync(path, 'utf8')).files ?? []).map((file) => [file.path, file]));
  const onDisk = manifestFiles(dir);
  const errors = [];
  for (const file of onDisk) {
    const entry = listed.get(file.path);
    if (!entry) errors.push(`manifest does not list ${file.path}`);
    else if (entry.sha256 !== file.sha256 || entry.bytes !== file.bytes) errors.push(`manifest hash of ${file.path} differs from the file`);
    listed.delete(file.path);
  }
  for (const missing of listed.keys()) errors.push(`manifest lists ${missing}, which is not on disk`);
  return errors;
}

function scriptErrors(run) {
  const recorded = new Map((run?.control?.scripts ?? []).map((s) => [s.path, s.sha256]));
  return scriptHashes().filter((s) => recorded.get(s.path) !== s.sha256).map((s) => `control script ${s.path} differs from the script validating now`);
}

/** Receipt verdict plus every on-disk check; `withManifest` is false only before it is written. */
export function verifyEvidence(dir, { withManifest = true } = {}) {
  const runPath = join(dir, 'run.json');
  if (!existsSync(runPath)) return validateRun(null, ['run.json missing']);
  const run = JSON.parse(readFileSync(runPath, 'utf8'));
  const errors = [...artifactErrors(dir, run), ...scriptErrors(run), ...(withManifest ? manifestErrors(dir) : [])];
  return validateRun(run, errors);
}
