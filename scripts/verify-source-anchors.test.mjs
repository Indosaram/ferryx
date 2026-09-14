import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, copyFileSync, rmSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const scripts = dirname(fileURLToPath(import.meta.url));
const documents = ["docs/HEADLESS_LINUX_SERVER_DEPLOYMENT_GUIDE.md", "docs/SOURCE_ANCHORS_VERIFICATION.md", "site/src/content/docs/privacy.md"];

function runFixture(text, sources = { "src/auth.rs": Array.from({ length: 20 }, (_, i) => i === 14 ? "const PAIRING_EXPIRY: Duration = Duration::from_secs(60);" : "use Duration;").join("\n") }) {
  const root = mkdtempSync(resolve(scripts, ".anchor-test-"));
  try {
    mkdirSync(resolve(root, "scripts"));
    copyFileSync(resolve(scripts, "verify-source-anchors.mjs"), resolve(root, "scripts/verify-source-anchors.mjs"));
    for (const [path, content] of [...documents.map((path, i) => [path, i === 0 ? text : ""]), ...Object.entries(sources)]) {
      mkdirSync(dirname(resolve(root, path)), { recursive: true });
      writeFileSync(resolve(root, path), content);
    }
    const result = spawnSync(process.execPath, ["scripts/verify-source-anchors.mjs"], { cwd: root, encoding: "utf8", timeout: 5000 });
    if (result.error) throw result.error;
    return { status: result.status, output: result.stdout + result.stderr };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

const cases = [
  ["ignores unrelated prose tokens", "`FERRYX_RUNTIME_DIR` is elsewhere (`src/auth.rs:1`), later (`:15`).", 0],
  ["does not turn absence into presence", "No `tracing_subscriber` (`src/auth.rs:1-20`).", 0],
  ["does not assert external shell mechanics", "Hostname uses `PATH` (`src/auth.rs:1`).", 0],
  ["preserves explicit formatted identifier assertions", "`src/auth.rs:15` (`PAIRING_EXPIRY = Duration::from_secs(60)`)", 0],
  ["detects drift despite generic Duration", "`src/auth.rs:1` (`PAIRING_EXPIRY = Duration::from_secs(60)`)", 1],
  ["detects nonexistent files", "`src/ghost.rs:1`", 1],
  ["detects out of range starts", "`src/auth.rs:999999`", 1],
  ["detects out of range ends", "`src/auth.rs:1-21`", 1],
  ["detects reversed ranges", "`src/auth.rs:15-1`", 1],
  ["checks every comma-separated range", "`src/auth.rs:1-2,15-21`", 1],
  ["searches the full explicit range", "`src/auth.rs:1-15` (`PAIRING_EXPIRY`)", 0],
  ["does not search gaps between explicit ranges", "`src/auth.rs:1-2,19-20` (`PAIRING_EXPIRY`)", 1],
  ["checks shorthand bounds", "`src/auth.rs:1`, `:21`", 1],
  ["checks basename shorthand bounds", "`src/auth.rs:1`, `auth.rs:21`", 1],
  ["rejects unbound shorthand", "`:21`", 1],
  ["rejects ambiguous basename shorthand", "`src/auth.rs:1`, `other/auth.rs:1`, `auth.rs:2`", 1, { "src/auth.rs": "one\ntwo", "other/auth.rs": "one\ntwo" }],
  ["establishes context from resolved basename", "`src/auth.rs:1`\n# Another section\n`auth.rs:15`, `:16`", 0],
  ["matches complete identifiers rather than substrings", "`src/auth.rs:1` (`PAIRING_EXPIRY`)", 1, { "src/auth.rs": "const PAIRING_EXPIRY_EXTRA = 60;" }],
  ["keeps nearby assertions separate", "`src/auth.rs:15` (`PAIRING_EXPIRY`), `src/auth.rs:1` (`Duration`)", 0],
  ["requires every distinctive identifier", "`src/auth.rs:15` (`PAIRING_EXPIRY + MISSING_TOKEN`)", 1],
  ["does not count trailing newline as a source line", "`src/one.rs:2`", 1, { "src/one.rs": "first\n" }],
  ["ignores captured and suppressed references", "```text\n`src/missing.rs:1`\n```\n<!-- anchor-check: off -->\n`src/missing.rs:1`\n<!-- anchor-check: on -->\n`src/auth.rs:1`", 0],
  ["ignores tilde fenced captured references", "~~~text\n`src/missing.rs:1`\n~~~\n`src/auth.rs:1`", 0],
];
for (const ext of ["rs", "ts", "tsx", "mjs", "toml", "json", "yml", "yaml", "astro"]) {
  cases.push([`checks missing ${ext} sources including dot directories`, `\`.github/missing.${ext}:1\``, 1]);
}
for (const [name, text, status, sources] of cases) {
  test(`${name} when cited in a document`, () => {
    // Given: synthetic sources and a documentation citation.
    // When: run the real verifier CLI in an isolated repository.
    const result = runFixture(text, sources);
    // Then: its exit status distinguishes the claimed contract.
    assert.equal(result.status, status, result.output);
    if (status === 1) assert.match(result.output, /BROKEN:|DRIFTED:/);
  });
}
