#!/usr/bin/env bun
// Verifies a cross-platform audit markdown doc:
//  1. every "- **Evidence**: `path:line` - `snippet`" citation resolves
//  2. every finding block carries a non-empty **Fix** field
// Usage: bun scripts/verify-audit-citations.mjs <doc.md> [repoRoot]
import { readFileSync, existsSync } from "node:fs";
import { resolve, join } from "node:path";

const docPath = process.argv[2];
if (!docPath) { console.error("usage: verify-audit-citations.mjs <doc.md> [repoRoot]"); process.exit(2); }
const repoRoot = resolve(process.argv[3] ?? process.cwd());
const doc = readFileSync(docPath, "utf8");
const lines = doc.split("\n");

const norm = (s) => s.replace(/\s+/g, " ").trim();
const fileCache = new Map();
const readLines = (p) => {
  if (!fileCache.has(p)) fileCache.set(p, existsSync(p) ? readFileSync(p, "utf8").split("\n") : null);
  return fileCache.get(p);
};

const citationRe = /^\s*-\s\*\*Evidence\*\*:\s*`([^`]+?):(\d+)`\s*(?:[\u2014-]\s*`([^`]*)`)?/;
const idRe = /^\s*-\s\*\*ID\*\*:\s*(\S+)/;
const fixRe = /^\s*-\s\*\*Fix\*\*:\s*(.*)$/;

const citationFailures = [];
let citationsOk = 0;

lines.forEach((line, i) => {
  const m = citationRe.exec(line);
  if (!m) return;
  const relPath = m[1], lineNo = Number(m[2]), snippetRaw = m[3];
  const abs = join(repoRoot, relPath);
  const src = readLines(abs);
  const where = docPath + ":" + (i + 1);
  if (!src) { citationFailures.push(where + " MISSING FILE " + relPath); return; }
  if (lineNo < 1 || lineNo > src.length) {
    citationFailures.push(where + " LINE OUT OF RANGE " + relPath + ":" + lineNo + " (file has " + src.length + " lines)");
    return;
  }
  const snippet = norm(snippetRaw ?? "").replace(/^\.\.\./, "").replace(/\.\.\.$/, "").trim();
  if (!snippet) { citationFailures.push(where + " EMPTY SNIPPET for " + relPath + ":" + lineNo); return; }
  const windowText = norm(src.slice(lineNo - 1, lineNo + 2).join(" "));
  if (!windowText.includes(snippet)) {
    citationFailures.push(where + " SNIPPET NOT FOUND at " + relPath + ":" + lineNo + "\n    want: " + snippet + "\n    got : " + norm(src[lineNo - 1]));
    return;
  }
  citationsOk += 1;
});

const findingStarts = [];
lines.forEach((line, i) => { if (idRe.test(line)) findingStarts.push(i); });
const findingFailures = [];
findingStarts.forEach((start, idx) => {
  const end = idx + 1 < findingStarts.length ? findingStarts[idx + 1] : lines.length;
  const block = lines.slice(start, end);
  const id = idRe.exec(lines[start])[1];
  const fixLine = block.find((l) => fixRe.test(l));
  const fixText = fixLine ? fixRe.exec(fixLine)[1].trim() : "";
  if (!fixText || fixText.length < 15) findingFailures.push(docPath + ":" + (start + 1) + " finding " + id + " has no actionable **Fix** (got: \"" + fixText + "\")");
  if (!block.some((l) => citationRe.test(l))) findingFailures.push(docPath + ":" + (start + 1) + " finding " + id + " has no **Evidence** citation");
});

console.log("citations resolved: " + citationsOk);
console.log("findings checked  : " + findingStarts.length);
if (citationFailures.length) {
  console.log("\nCITATION FAILURES (" + citationFailures.length + "):");
  for (const f of citationFailures) console.log("  " + f);
}
if (findingFailures.length) {
  console.log("\nFINDING FAILURES (" + findingFailures.length + "):");
  for (const f of findingFailures) console.log("  " + f);
}
const total = citationFailures.length + findingFailures.length;
console.log(total === 0 ? "\nRESULT: PASS" : "\nRESULT: FAIL (" + total + " problems)");
process.exit(total === 0 ? 0 : 1);
