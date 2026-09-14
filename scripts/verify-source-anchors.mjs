#!/usr/bin/env node
// Structural citation validation, not proof of prose claims. Only the local notation
// `path:line` (`CODE`) asserts identifiers. See docs/ANCHOR_CHECKER_REPAIR_EVIDENCE.md.
import { readFileSync, existsSync } from "node:fs";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DELIVERABLES = [
  "docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md",
  "docs/SOURCE_ANCHORS_VERIFICATION.md",
  "site/src/content/docs/privacy.md",
];
// Full references may be unquoted. Shorthand must be inline code to avoid ports,
// times, and other prose numbers. Never resolve a basename by searching the tree.
const ANCHOR = /(?<![A-Za-z0-9_/.-])([A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]+)*\.(?:rs|ts|tsx|mjs|toml|json|yml|yaml|astro)):(\d+(?:-\d+)?(?:,\d+(?:-\d+)?)*)|`:(\d+(?:-\d+)?(?:,\d+(?:-\d+)?)*)`/g;
const WINDOW = 3;
const identifiers = (text) => [...text.matchAll(/[A-Za-z_][A-Za-z0-9_]*/g)]
  .map((m) => m[0]);

let checked = 0;
let assertions = 0;
const failures = [];
for (const rel of DELIVERABLES) {
  const abs = resolve(ROOT, rel);
  if (!existsSync(abs)) {
    failures.push(`${rel}: deliverable missing`);
    continue;
  }
  const lines = readFileSync(abs, "utf8").split("\n");
  let fence = "";
  let suppressed = false;
  let previousPath;
  const established = new Map();
  lines.forEach((line, i) => {
    const marker = /^\s*(`{3,}|~{3,})/.exec(line)?.[1];
    if (marker) {
      if (!fence) fence = marker;
      else if (marker[0] === fence[0] && marker.length >= fence.length) fence = "";
      return;
    }
    if (fence) return;
    if (/<!--\s*anchor-check:\s*off\s*-->/.test(line)) suppressed = true;
    if (/<!--\s*anchor-check:\s*on\s*-->/.test(line)) suppressed = false;
    if (suppressed) return;
    if (/^\s*#/.test(line)) previousPath = undefined;

    for (const m of line.matchAll(ANCHOR)) {
      const [, path, numbered, shorthand] = m;
      if (path && !path.includes("/") && line[m.index - 1] !== "`") continue;
      const spec = numbered ?? shorthand;
      let srcRel = path;
      if (path?.includes("/")) {
        const candidates = established.get(basename(path)) ?? new Set();
        candidates.add(path);
        established.set(basename(path), candidates);
        previousPath = path;
      } else if (path) {
        const candidates = established.get(path);
        srcRel = candidates?.size === 1 ? [...candidates][0] : undefined;
        previousPath = srcRel;
      } else {
        srcRel = previousPath;
      }
      checked += 1;
      const where = `${rel}:${i + 1} -> ${srcRel ?? path ?? "(shorthand)"}:${spec}`;
      if (!srcRel) {
        failures.push(`${where}  BROKEN: shorthand has no unambiguous established source path`);
        continue;
      }
      const srcAbs = resolve(ROOT, srcRel);
      if (!existsSync(srcAbs)) {
        failures.push(`${where}  BROKEN: source file does not exist`);
        continue;
      }
      const content = readFileSync(srcAbs, "utf8");
      const srcLines = content === "" ? [] : content.replace(/\n$/, "").split("\n");
      const ranges = spec.split(",").map((part) => {
        const [start, end = start] = part.split("-").map(Number);
        return { start, end };
      });
      if (ranges.some(({ start, end }) => !Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 1 || end < start || end > srcLines.length)) {
        failures.push(`${where}  BROKEN: line or range out of range/reversed (file has ${srcLines.length} lines)`);
        continue;
      }

      // Only code directly attached to this citation is an assertion; never collect
      // tokens from the surrounding sentence, table row, or another citation.
      const suffix = line.slice(m.index + m[0].length);
      const code = (shorthand ? /^\s*\(`([^`]+)`\)/ : /^`\s*\(`([^`]+)`\)/).exec(suffix)?.[1];
      if (code === undefined) continue;
      const tokens = [...new Set(identifiers(code))];
      const screaming = tokens.filter((t) => /^[A-Z][A-Z0-9_]{3,}$/.test(t));
      const required = screaming.length > 0 ? screaming : tokens;
      if (required.length === 0) continue;
      assertions += 1;
      const explicitRange = /[-,]/.test(spec);
      const source = ranges.map(({ start, end }) => srcLines.slice(
        Math.max(0, start - 1 - (explicitRange ? 0 : WINDOW)),
        end + (explicitRange ? 0 : WINDOW),
      ).join("\n")).join("\n");
      const found = new Set(identifiers(source));
      const missing = required.filter((token) => !found.has(token));
      if (missing.length) {
        failures.push(`${where}  DRIFTED: missing ${JSON.stringify(missing)} ${explicitRange ? "in cited ranges" : `within +/-${WINDOW} lines`}`);
      }
    }
  });
}

if (failures.length > 0) {
  console.error(`FAIL  ${failures.length} of ${checked} references did not verify (${assertions} explicit token assertions):\n`);
  for (const failure of failures) console.error(`  ${failure}`);
  process.exit(1);
}
console.log(`OK  ${checked} structural references verified across ${DELIVERABLES.length} deliverables; ${assertions} explicit token assertions verified (prose claims not checked)`);
