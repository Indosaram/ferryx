#!/usr/bin/env bun

import { existsSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const RULE_FILE = join(REPO_ROOT, "scripts/native-terminal-thread-policy.rules.yml");
const DEFAULT_TARGETS = [join(REPO_ROOT, "src-tauri/src")];

function resolveAstGrepBinary() {
  for (const candidate of ["sg", "ast-grep"]) {
    const probe = Bun.spawnSync([candidate, "--version"], { stdout: "pipe", stderr: "pipe" });
    if (probe.success) return candidate;
  }
  return null;
}

export async function scanThreadPolicy(targets = DEFAULT_TARGETS) {
  if (!existsSync(RULE_FILE)) {
    throw new Error(`thread policy rule file is missing: ${RULE_FILE}`);
  }
  const binary = resolveAstGrepBinary();
  if (!binary) {
    throw new Error("ast-grep is required for the native terminal thread policy (brew install ast-grep)");
  }
  const present = targets.filter((target) => existsSync(target));
  if (present.length === 0) {
    throw new Error(`no scan target exists: ${targets.join(", ")}`);
  }

  const proc = Bun.spawnSync([binary, "scan", "--rule", RULE_FILE, "--json=stream", ...present], {
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
