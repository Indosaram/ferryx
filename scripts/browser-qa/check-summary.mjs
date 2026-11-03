#!/usr/bin/env bun
// Fails the job on a false check in a browser QA summary.json.
//
// Usage: bun scripts/browser-qa/check-summary.mjs <dir> [<dir>...]
//
// Reads every summary.json under the given dirs. A failure is a `false` entry of any `checks`
// object outside `steps` (per-step action records are evidence, not verdicts), unless the object
// that holds `checks` lists the name in its `informational` array, and a top-level `pass: false`.
// A dir without a summary.json fails too. Prints one line per check and exits 1 on any failure.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

/** Every summary.json at or below `dir`. */
function summaries(dir) {
  const found = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) found.push(...summaries(path));
    else if (name === "summary.json") found.push(path);
  }
  return found;
}

/** [path, name, value, informational] for each check in `node`, skipping `steps`. */
export function collectChecks(node, path = []) {
  if (!node || typeof node !== "object") return [];
  const rows = [];
  if (!Array.isArray(node) && node.checks && typeof node.checks === "object" && !Array.isArray(node.checks)) {
    const informational = Array.isArray(node.informational) ? node.informational : [];
    for (const [name, value] of Object.entries(node.checks)) {
      rows.push({ path: [...path, "checks", name].join("."), value, informational: informational.includes(name) });
    }
  }
  for (const [key, child] of Object.entries(node)) {
    if (key === "steps" || key === "checks") continue;
    rows.push(...collectChecks(child, [...path, key]));
  }
  return rows;
}

if (import.meta.main) {
  const dirs = process.argv.slice(2);
  if (!dirs.length) {
    console.error("usage: check-summary.mjs <dir> [<dir>...]");
    process.exit(2);
  }
  let failed = 0;
  for (const dir of dirs) {
    const files = existsSync(dir) ? summaries(dir) : [];
    if (!files.length) {
      console.log(`FAIL ${dir}: no summary.json`);
      failed += 1;
      continue;
    }
    for (const file of files) {
      const name = relative(dir, file) || file;
      let summary;
      try {
        summary = JSON.parse(readFileSync(file, "utf8"));
      } catch (error) {
        console.log(`FAIL ${name}: unreadable (${error.message})`);
        failed += 1;
        continue;
      }
      if (summary.pass === false) {
        console.log(`FAIL ${name}: pass=false`);
        failed += 1;
      }
      for (const row of collectChecks(summary)) {
        const ok = row.value === true;
        const verdict = ok ? "ok  " : row.informational ? "info" : "FAIL";
        if (!ok && !row.informational) failed += 1;
        console.log(`${verdict} ${name}: ${row.path} = ${JSON.stringify(row.value)}`);
      }
    }
  }
  console.log(failed ? `${failed} failing check(s)` : "all checks pass");
  process.exit(failed ? 1 : 0);
}
