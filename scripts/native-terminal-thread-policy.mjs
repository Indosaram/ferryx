#!/usr/bin/env bun

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const RULE_FILE = join(REPO_ROOT, "scripts/native-terminal-thread-policy.rules.yml");
const DEFAULT_TARGETS = [join(REPO_ROOT, "src-tauri/src")];

// @ast-grep/cli is a pinned devDependency, so the checked-in binary must be preferred over a
// system install: CI runners install the devDependency with `bun install` and have no ast-grep
// on PATH. Falling back to PATH keeps Homebrew/system installs working on developer machines.
function localAstGrepCandidates() {
  const binDir = join(REPO_ROOT, "node_modules", ".bin");
  return process.platform === "win32"
    ? [join(binDir, "ast-grep.cmd"), join(binDir, "ast-grep.exe")]
    : [join(binDir, "ast-grep"), join(binDir, "sg")];
}

function probeBinary(candidate) {
  try {
    return Bun.spawnSync([candidate, "--version"], { stdout: "pipe", stderr: "pipe" }).success;
  } catch {
    // Bun.spawnSync throws when the executable cannot be found at all; a candidate that cannot
    // run must not abort the search for one that can.
    return false;
  }
}

function resolveAstGrepBinary() {
  for (const candidate of localAstGrepCandidates()) {
    if (existsSync(candidate) && probeBinary(candidate)) return candidate;
  }
  for (const candidate of ["sg", "ast-grep"]) {
    if (probeBinary(candidate)) return candidate;
  }
  return null;
}

// Every rule path matches a GPU call nested inside either a direct `run_on_main_thread(` call or
// the project's `dispatch_render_on_main_thread(` wrapper, so a file that mentions neither
// wrapper cannot contain a match. The rule is a conjunction of a call pattern and an enclosing
// wrapper, and no `--rule` may relax that, so parsing the other files can only ever produce an
// empty result. Skipping them avoids parsing all 729 Rust files here: the full scan costs
// ~3.8s, which left this gate within a second of Bun's 5s per-test deadline and made it fail
// whenever the suite ran it under load.
const RULE_ENCLOSING_MARKERS = ["run_on_main_thread(", "dispatch_render_on_main_thread("];

function confinesMatches(file) {
  try {
    const source = readFileSync(file, "utf8");
    return RULE_ENCLOSING_MARKERS.some((marker) => source.includes(marker));
  } catch {
    // An unreadable file must not be silently exempted from the gate.
    return true;
  }
}

function collectCandidateFiles(target) {
  const stat = statSync(target);
  if (stat.isFile()) return [target];
  const candidates = [];
  for (const entry of readdirSync(target, { withFileTypes: true })) {
    const child = join(target, entry.name);
    if (entry.isDirectory()) candidates.push(...collectCandidateFiles(child));
    else if (entry.isFile() && entry.name.endsWith(".rs") && confinesMatches(child)) candidates.push(child);
  }
  return candidates;
}

export async function scanThreadPolicy(targets = DEFAULT_TARGETS) {
  if (!existsSync(RULE_FILE)) {
    throw new Error(`thread policy rule file is missing: ${RULE_FILE}`);
  }
  const binary = resolveAstGrepBinary();
  if (!binary) {
    throw new Error(
      "ast-grep is required for the native terminal thread policy (bun install for the pinned @ast-grep/cli devDependency, or brew install ast-grep)",
    );
  }
  const present = targets.filter((target) => existsSync(target));
  if (present.length === 0) {
    throw new Error(`no scan target exists: ${targets.join(", ")}`);
  }
  // Scanning nothing must still report nothing: ast-grep exits non-zero when it is handed no
  // path, so a tree with no enclosing-wrapper call is answered directly.
  const candidates = present.flatMap(collectCandidateFiles);
  if (candidates.length === 0) return [];

  const proc = Bun.spawnSync([binary, "scan", "--rule", RULE_FILE, "--json=stream", ...candidates], {
    stdout: "pipe",
    stderr: "pipe",
  });
  const stdout = proc.stdout.toString();
  const stderr = proc.stderr.toString();
  if (proc.exitCode !== 0 && stdout.trim() === "") {
    throw new Error(`ast-grep scan failed (exit ${proc.exitCode}): ${stderr.trim()}`);
  }

  return stdout
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => JSON.parse(line))
    .map((match) => ({
      file: relative(REPO_ROOT, match.file),
      line: (match.range?.start?.line ?? 0) + 1,
      text: (match.text ?? "").trim(),
      message: match.message ?? "",
    }));
}

function reportAndExit(violations) {
  if (violations.length === 0) {
    console.log("native terminal thread policy: OK (no GPU acquisition inside run_on_main_thread)");
    process.exit(0);
  }
  console.error(`native terminal thread policy: ${violations.length} violation(s)\n`);
  for (const violation of violations) {
    console.error(`${violation.file}:${violation.line}: ${violation.text}`);
    if (violation.message) console.error(`  -> ${violation.message}`);
  }
  console.error("\nGPU acquisition must run on the GPU worker, never inside a run_on_main_thread closure.");
  process.exit(1);
}

if (import.meta.main) {
  const argv = process.argv.slice(2);
  const targets = argv.length > 0 ? argv.map((arg) => resolve(arg)) : DEFAULT_TARGETS;
  try {
    reportAndExit(await scanThreadPolicy(targets));
  } catch (error) {
    console.error(`native terminal thread policy: ${error.message}`);
    process.exit(2);
  }
}
