#!/usr/bin/env bun

// Multi-window policy: production code must not look up the desktop window by the literal
// "main" label. Every window lookup goes through src-tauri/src/window_registry.rs (caller window,
// session owner window, focused window or primary window), otherwise a second window silently
// routes its rendering, input or events to the first one.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_ROOT = join(REPO_ROOT, "src-tauri/src");
const ALLOWED_FILES = new Set(["window_registry.rs"]);
const LOOKUP = /\.get_(?:webview_)?window\(\s*"main"\s*\)|label\s*==\s*"main"/;

function isTestPath(path) {
  return /(^|\/)tests?(\/|\.rs$)|_tests?\.rs$/.test(path);
}

function rustFiles(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...rustFiles(path));
    else if (name.endsWith(".rs")) out.push(path);
  }
  return out;
}

// Lines inside a `#[cfg(test)]` module are test code. The module is assumed to run to the end
// of the file, which is how this repository lays out inline test modules.
function productionLines(source) {
  const lines = source.split("\n");
  const testStart = lines.findIndex((line) => line.trim() === "#[cfg(test)]");
  return testStart === -1 ? lines : lines.slice(0, testStart);
}

export function scanMultiWindowPolicy(root = DEFAULT_ROOT) {
  const violations = [];
  for (const file of rustFiles(root)) {
    const rel = relative(root, file).split("\\").join("/");
    if (ALLOWED_FILES.has(rel) || isTestPath(rel)) continue;
    productionLines(readFileSync(file, "utf8")).forEach((line, index) => {
      if (line.trim().startsWith("//")) return;
      if (LOOKUP.test(line)) violations.push(`${rel}:${index + 1}: ${line.trim()}`);
    });
  }
  return violations;
}

if (import.meta.main) {
  const violations = scanMultiWindowPolicy(process.argv[2] ? resolve(process.argv[2]) : DEFAULT_ROOT);
  if (violations.length > 0) {
    console.error("multi-window policy: window looked up by the \"main\" label outside window_registry.rs:");
    for (const v of violations) console.error(`  ${v}`);
    console.error("Use window_registry::{window_for_session, focused_window, primary_window} or the injected caller window.");
    process.exit(1);
  }
  console.log("multi-window policy: ok");
}

