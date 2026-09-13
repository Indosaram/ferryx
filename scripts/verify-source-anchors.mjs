#!/usr/bin/env node
// Verifies that every `path/to/file.rs:NN` anchor in the sa-docs deliverables resolves to a
// real line in the repository, and that anchors which embed a quoted token actually find that
// token at (or within a small window of) the cited line.
//
// Exits 0 when every anchor resolves, 1 otherwise, printing each failure.
//
// This exists so C3's "every factual claim traces to a source file:line" is a check that can
// FAIL, rather than a one-time manual reading. See the mutation proof in
// docs/SOURCE_ANCHORS_VERIFICATION.md.

import { readFileSync, existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const DELIVERABLES = [
  "docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md",
  "docs/SOURCE_ANCHORS_VERIFICATION.md",
  "site/src/content/docs/privacy.md",
];

// `src-tauri/src/remote/auth.rs:196`. Requires a directory separator: a bare `auth.rs:188`
// is prose shorthand referring back to an already-established path, not a resolvable anchor.
const ANCHOR = /([A-Za-z0-9_][A-Za-z0-9_/.-]*\/[A-Za-z0-9_.-]+\.(?:rs|ts|tsx|mjs|toml)):(\d+)/g;
// A quoted token on the same markdown line, e.g. `PAIRING_EXPIRY` or "..." text.
const QUOTED = /`([^`\n]{3,80})`|\*"([^"\n]{3,80})"\*/g;
const WINDOW = 3;

// Compare on IDENTIFIERS, not on formatted source text. Docs legitimately write
// `DEVICE_IDLE_EXPIRY_SECS = 30 * 24 * 60 * 60` while the source carries a type annotation
// (`: u64 =`), and write a config key as `"port"` where the source has a bare `port:`.
// Matching whole formatted expressions flags both as drift, which is a false alarm.
// Only SCREAMING_CASE / camelCase / PascalCase identifiers are treated as claims about
// source text. Lowercase prose words are not, and neither are branch names like
// `sa-watchdog`, which appear in the cross-track drift table as the *cause* of a shift
// rather than as something expected to be found in the cited file.
const identifiers = (text) => {
  if (/^sa-[a-z-]+$/.test(text.trim())) return [];
  return [...text.matchAll(/[A-Za-z_][A-Za-z0-9_]{3,}/g)]
    .map((m) => m[0])
    .filter((w) => /_|[a-z][A-Z]|^[A-Z]/.test(w));
};

let checked = 0;
const failures = [];

for (const rel of DELIVERABLES) {
  const abs = resolve(ROOT, rel);
  if (!existsSync(abs)) {
    failures.push(`${rel}: deliverable missing`);
    continue;
  }
  const lines = readFileSync(abs, "utf8").split("\n");

  // Two kinds of text in these documents are not claims about the source and must not be
  // checked: fenced blocks, which hold verbatim captured command output, and regions marked
  // with `anchor-check: off`, which document deliberately-broken anchors as evidence. Without
  // these exclusions the mutation proof below would permanently fail its own checker.
  let fenced = false;
  let suppressed = false;

  lines.forEach((line, i) => {
    if (/^\s*```/.test(line)) {
      fenced = !fenced;
      return;
    }
    if (/<!--\s*anchor-check:\s*off\s*-->/.test(line)) suppressed = true;
    if (/<!--\s*anchor-check:\s*on\s*-->/.test(line)) suppressed = false;
    if (fenced || suppressed) return;

    for (const m of line.matchAll(ANCHOR)) {
      const [, srcRel, lineNoRaw] = m;
      const lineNo = Number(lineNoRaw);
      checked += 1;
      const where = `${rel}:${i + 1} -> ${srcRel}:${lineNo}`;

      const srcAbs = resolve(ROOT, srcRel);
      if (!existsSync(srcAbs)) {
        failures.push(`${where}  BROKEN: source file does not exist`);
        continue;
      }
      const srcLines = readFileSync(srcAbs, "utf8").split("\n");
      if (lineNo < 1 || lineNo > srcLines.length) {
        failures.push(
          `${where}  BROKEN: line out of range (file has ${srcLines.length} lines)`,
        );
        continue;
      }

      // If the doc line quotes a token, require it near the cited line. This is what turns a
      // silently-drifted line number into a failure instead of a pass.
      const tokens = [
        ...new Set(
          [...line.matchAll(QUOTED)]
            .map((q) => (q[1] ?? q[2] ?? "").trim())
            .filter((t) => t && !/^[a-zA-Z0-9_/.-]+\.(rs|ts|tsx|mjs|toml):\d/.test(t))
            .flatMap(identifiers),
        ),
      ];
      if (tokens.length === 0) continue;

      // Require the DISTINCTIVE token, not just any token. `PAIRING_EXPIRY = Duration::…`
      // yields both `PAIRING_EXPIRY` and `Duration`; the latter occurs in nearly every file's
      // imports, so accepting any match lets a generic word mask a genuinely drifted line.
      // SCREAMING_CASE names are specific to their definition site, so when one is present it
      // is the token that must be found.
      const screaming = tokens.filter((t) => /^[A-Z][A-Z0-9_]{3,}$/.test(t));
      const required = screaming.length > 0 ? screaming : tokens;

      const lo = Math.max(0, lineNo - 1 - WINDOW);
      const hi = Math.min(srcLines.length, lineNo + WINDOW);
      const window = srcLines.slice(lo, hi).join("\n");
      if (!required.some((t) => window.includes(t))) {
        failures.push(
          `${where}  DRIFTED: none of [${required
            .slice(0, 6)
            .map((t) => JSON.stringify(t))
            .join(", ")}] found within +/-${WINDOW} lines`,
        );
      }
    }
  });
}

if (failures.length > 0) {
  console.error(`FAIL  ${failures.length} of ${checked} anchors did not verify:\n`);
  for (const f of failures) console.error(`  ${f}`);
  process.exit(1);
}
console.log(`OK  ${checked} anchors verified across ${DELIVERABLES.length} deliverables`);
