#!/usr/bin/env node
// Fails when owner-private paths or relay topology appear in the tracked tree.
//
// Patterns: the owner's macOS home and volume, the Linux and Windows build-host homes, and the
// Cloudflare tunnel origin host. Excluded like the todo-38/46 QA grep: .omo, fixture and testdata
// trees, and test files (`*test*`, `*_tests.rs`). Rust hits are allowed only inside `#[cfg(test)]`
// modules (sample inputs that never touch the filesystem). AGENTS.md is a declared exception: its
// host table names each build host's real canonical clone.
//
// Usage: node scripts/qa/owner-path-check.mjs [repo-root]   (exit 0 clean, 1 hits, 2 error)
import { spawnSync } from "node:child_process";
import { readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

// The bracketed final letters keep this source line from matching its own pattern.
export const OWNER_PATH_PATTERN = /\/Users\/ind[o]|\/Volumes\/T9-Ma[c]|\/home\/ind[o]|Users.soo[k]|C:\/Users\/soo[k]|cfargotunnel[.]com/;
export const EXCLUDES = [":!.omo", ":!docs/evidence", ":!**/fixtures/**", ":!**/__fixtures__/**", ":!**/testdata/**", ":!*test*", ":!*_tests.rs"];
export const DECLARED_EXCEPTIONS = new Map([["AGENTS.md", "agent host table (owner-declared exception)"]]);

/** 1-based line numbers that sit inside a top-level `#[cfg(test)] mod x { ... }` or `mod x;`. */
export function cfgTestLines(source) {
  const lines = source.split("\n");
  const inTest = new Set();
  for (let i = 0; i < lines.length; i++) {
    if (!lines[i].startsWith("#[cfg(test)]")) continue;
    let j = i + 1;
    while (j < lines.length && lines[j].startsWith("#[")) j++;
    if (/^(pub(\([^)]*\))? )?mod \w+;/.test(lines[j] ?? "")) {
      inTest.add(j + 1);
      continue;
    }
    if (!/^(pub(\([^)]*\))? )?mod \w+\s*\{/.test(lines[j] ?? "")) continue;
    let depth = 0;
    for (let k = j; k < lines.length; k++) {
      for (const ch of lines[k]) depth += ch === "{" ? 1 : ch === "}" ? -1 : 0;
      inTest.add(k + 1);
      if (depth === 0) {
        i = k;
        break;
      }
    }
  }
  return inTest;
}

export function findOwnerPaths(root) {
  // Tracked plus untracked-but-not-ignored files, so a new file is caught before it is staged.
  const list = spawnSync("git", ["-C", root, "ls-files", "-z", "-co", "--exclude-standard", "--", ".", ...EXCLUDES], { encoding: "utf8", maxBuffer: 64 << 20 });
  if (list.status !== 0) throw new Error(`git ls-files failed: ${list.stderr.trim()}`);
  const hits = [];
  const allowed = [];
  for (const file of new Set(list.stdout.split("\0").filter(Boolean))) {
    const abs = join(root, file);
    if (!statSync(abs, { throwIfNoEntry: false })?.isFile()) continue; // deleted, or a submodule gitlink
    const bytes = readFileSync(abs);
    if (bytes.includes(0)) continue; // binary
    const source = bytes.toString("utf8");
    if (!OWNER_PATH_PATTERN.test(source)) continue;
    const testLines = file.endsWith(".rs") ? cfgTestLines(source) : new Set();
    source.split("\n").forEach((text, i) => {
      if (!OWNER_PATH_PATTERN.test(text)) return;
      const line = i + 1;
      const reason = DECLARED_EXCEPTIONS.get(file) ?? (testLines.has(line) ? "#[cfg(test)] sample input" : null);
      (reason ? allowed : hits).push({ file, line, text: text.trim().slice(0, 160), ...(reason ? { reason } : {}) });
    });
  }
  return { hits, allowed };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    const { hits, allowed } = findOwnerPaths(resolve(process.argv[2] ?? "."));
    const files = new Map();
    for (const a of allowed) files.set(a.file, `${(files.get(a.file)?.split(" ")[0] ?? 0) * 1 + 1} ${a.reason}`);
    for (const [file, summary] of files) console.log(`allowed ${file}: ${summary}`);
    for (const h of hits) console.log(`OWNER_PATH ${h.file}:${h.line}: ${h.text}`);
    console.log(`owner-path-check: ${hits.length} hit(s), ${allowed.length} allowed`);
    process.exit(hits.length ? 1 : 0);
  } catch (error) {
    console.error(String(error?.message ?? error));
    process.exit(2);
  }
}
