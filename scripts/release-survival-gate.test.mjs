import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { writeSurvivalFixture } from "./fixtures/survival-fixture.mjs";
import { appendSurvivalReport } from "./lib/release-changelog.mjs";
import {
  SURVIVAL_SCENARIOS,
  SurvivalGateError,
  assertSurvivalGate,
  evaluateSurvivalGate,
  survivalScenariosFor,
} from "./lib/release-survival.mjs";
import { publishRelease } from "./release-local.mjs";

const COMMIT = "5d5499806a1b207849778f488b4e3e7b821a751b";
const OTHER = "0".repeat(40);

function withTmp(fn) {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-survival-gate-"));
  try {
    return fn(tmp);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

function refused(runDir, code, opts = {}) {
  assert.throws(
    () => assertSurvivalGate({ releaseDir: runDir, commitSha: COMMIT, ...opts }),
    (error) => error instanceof SurvivalGateError && error.code === code && error.message.startsWith(`${code}: `),
  );
}

test("gate scenario set follows summary.mjs: windows skips Q03b and Q08", () => {
  assert.deepEqual(survivalScenariosFor("mac"), SURVIVAL_SCENARIOS);
  assert.deepEqual(survivalScenariosFor("linux"), SURVIVAL_SCENARIOS);
  assert.deepEqual(survivalScenariosFor("windows"), SURVIVAL_SCENARIOS.filter((id) => id !== "Q03b" && id !== "Q08"));
});

test("all-PASS fixture is accepted and writes survival-report.md", () => withTmp((runDir) => {
  writeSurvivalFixture(runDir, COMMIT);
  const { result, report, reportPath } = assertSurvivalGate({ releaseDir: runDir, commitSha: COMMIT });
  assert.equal(result.ok, true);
  assert.equal(result.waiver, null);
  assert.equal(readFileSync(reportPath, "utf8"), report);
  assert.match(report, /^## Session survival/);
  assert.match(report, /\| Q01 \| PASS \(2\/2 sessions\) \| PASS \(2\/2 sessions\) \| PASS \(2\/2 sessions\) \|/);
  assert.match(report, /\| Q03b \| PASS \(2\/2 sessions\) \| PASS \(2\/2 sessions\) \| n\/a \|/);
  assert.match(report, /Footprint \(footprint\.json\): present\./);
  assert.doesNotMatch(report, /waiver/i);
}));

test("missing summary is refused with SURVIVAL_SUMMARY_MISSING", () => withTmp((runDir) => {
  writeSurvivalFixture(runDir, COMMIT, { platforms: ["mac", "linux"] });
  refused(runDir, "SURVIVAL_SUMMARY_MISSING");
}));

test("a FAIL row is refused with SURVIVAL_FAIL and shown in the report", () => withTmp((runDir) => {
  writeSurvivalFixture(runDir, COMMIT, {
    mutate: (summary, platform) => {
      if (platform === "linux") summary.scenarios.find((s) => s.id === "Q11").status = "FAIL";
    },
  });
  refused(runDir, "SURVIVAL_FAIL");
  const report = readFileSync(join(runDir, "survival", "survival-report.md"), "utf8");
  assert.match(report, /\| Q11 \| PASS \(2\/2 sessions\) \| FAIL \(2\/2 sessions\) \|/);
  assert.match(report, /SURVIVAL_FAIL: Q11 FAIL on linux/);
}));

test("a stale commit is refused with SURVIVAL_STALE_COMMIT", () => withTmp((runDir) => {
  writeSurvivalFixture(runDir, COMMIT, { mutate: (summary, platform) => { if (platform === "windows") summary.commit = OTHER; } });
  refused(runDir, "SURVIVAL_STALE_COMMIT");
}));

test("a summary without a commit field is refused as stale", () => withTmp((runDir) => {
  writeSurvivalFixture(runDir, COMMIT, { mutate: (summary) => { delete summary.commit; } });
  refused(runDir, "SURVIVAL_STALE_COMMIT");
}));

test("missing footprint.json is refused with SURVIVAL_FOOTPRINT_MISSING", () => withTmp((runDir) => {
  writeSurvivalFixture(runDir, COMMIT, { footprint: false });
  refused(runDir, "SURVIVAL_FOOTPRINT_MISSING");
}));

test("an owner waiver is accepted for missing evidence and recorded in the report", () => withTmp((runDir) => {
  writeSurvivalFixture(runDir, COMMIT, { platforms: ["mac", "linux"], footprint: false });
  const { result, report } = assertSurvivalGate({ releaseDir: runDir, commitSha: COMMIT, waiver: "footprint pending todo 17" });
  assert.equal(result.ok, true);
  assert.equal(result.waiver, "footprint pending todo 17");
  assert.match(report, /\*\*Survival waiver \(owner override\):\*\* footprint pending todo 17/);
  assert.match(report, /- SURVIVAL_FOOTPRINT_MISSING: /);
  assert.match(report, /- SURVIVAL_SUMMARY_MISSING: /);
}));

test("a waiver never covers a FAIL or a stale commit", () => withTmp((runDir) => {
  writeSurvivalFixture(runDir, COMMIT, { mutate: (summary, platform) => { if (platform === "mac") summary.scenarios.find((s) => s.id === "Q14").status = "FAIL"; } });
  refused(runDir, "SURVIVAL_FAIL", { waiver: "host down" });
  rmSync(join(runDir, "survival"), { recursive: true });
  writeSurvivalFixture(runDir, OTHER);
  refused(runDir, "SURVIVAL_STALE_COMMIT", { waiver: "host down" });
  assert.equal(evaluateSurvivalGate({ releaseDir: runDir, commitSha: COMMIT, waiver: "   " }).waiver, null);
}));

test("appendSurvivalReport puts the report in the release body once", () => {
  const once = appendSurvivalReport("# Ferryx release v1\n\n## Features\n\n- x\n", "## Session survival\n\n| a |\n");
  assert.equal(once, "# Ferryx release v1\n\n## Features\n\n- x\n\n## Session survival\n\n| a |\n");
  assert.equal(appendSurvivalReport(once, "## Session survival\n\n| b |\n"), "# Ferryx release v1\n\n## Features\n\n- x\n\n## Session survival\n\n| b |\n");
});

// --- drafting step (publishRelease) ---

function digestDirectory(directory) {
  const hash = createHash("sha256");
  const visit = (current) => {
    for (const name of readdirSync(current).sort()) {
      const path = join(current, name);
      if (statSync(path).isDirectory()) visit(path);
      else hash.update(path.slice(directory.length + 1)).update("\0").update(readFileSync(path));
    }
  };
  visit(directory);
  return hash.digest("hex");
}

/** An assembled run dir plus a fake gh that logs its calls and records the draft notes. */
function assembledRun(tmp) {
  const runDir = join(tmp, "run");
  const publishDir = join(runDir, "publish");
  mkdirSync(publishDir, { recursive: true });
  const plan = {
    schemaVersion: 1, runId: "rel-run-survival-1", repo: "Indosaram/ferryx", commitSha: COMMIT,
    tag: "v2026.1024.1", appVersion: "2026.1024.1", msixVersion: "2026.1024.1.0",
    channels: { store: true, nsisMigration: false }, requiredTargets: ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"],
    toolchains: { node: ">=22.0.0", bun: ">=1.4.0", zig: "0.17.0" }, sourceDateEpoch: 1788868800, createdAt: "2026-09-08T12:00:00.000Z",
  };
  writeFileSync(join(runDir, "plan.json"), JSON.stringify(plan));
  writeFileSync(join(publishDir, "latest.json"), JSON.stringify({ version: plan.appVersion }));
  writeFileSync(join(runDir, "RELEASE_NOTES.md"), "# Ferryx release v2026.1024.1\n");
  const configPath = join(runDir, "hosts.json");
  writeFileSync(configPath, "{}\n");
  const planDigest = createHash("sha256").update(readFileSync(join(runDir, "plan.json"))).digest("hex");
  writeFileSync(join(runDir, "prepare-state.json"), JSON.stringify({
    schemaVersion: 1, runId: plan.runId, planDigest,
    configDigest: createHash("sha256").update(readFileSync(configPath)).digest("hex"), configPath,
    commitSha: COMMIT, sourceDateEpoch: plan.sourceDateEpoch, ghosttyPin: "a".repeat(40), updaterPublicKey: "fixture",
  }));
  for (const dir of ["receipts", "artifacts"]) mkdirSync(join(runDir, dir), { recursive: true });
  writeFileSync(join(runDir, "assembly-state.json"), JSON.stringify({
    schemaVersion: 1, planDigest, receiptsDigest: digestDirectory(join(runDir, "receipts")),
    artifactsDigest: digestDirectory(join(runDir, "artifacts")), publishDigest: digestDirectory(publishDir),
  }));

  const callLog = join(tmp, "gh-calls.log");
  const notesCopy = join(tmp, "draft-notes.md");
  const assets = join(tmp, "assets");
  const gh = join(tmp, "fake-gh.mjs");
  writeFileSync(gh, `#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
const args = process.argv.slice(2);
fs.appendFileSync(${JSON.stringify(callLog)}, args.join(" ") + "\\n");
const [cmd, sub] = args;
if (cmd !== "release") process.exit(1);
if (sub === "view") { process.stderr.write("release not found\\n"); process.exit(1); }
if (sub === "create") { fs.copyFileSync(args[args.indexOf("--notes-file") + 1], ${JSON.stringify(notesCopy)}); process.exit(0); }
if (sub === "upload") {
  fs.mkdirSync(${JSON.stringify(assets)}, { recursive: true });
  for (const f of args.slice(3).filter((a) => a.includes(path.sep) && fs.existsSync(a))) fs.copyFileSync(f, path.join(${JSON.stringify(assets)}, path.basename(f)));
  process.exit(0);
}
if (sub === "download") {
  const out = args[args.indexOf("--dir") + 1];
  fs.mkdirSync(out, { recursive: true });
  for (const f of fs.readdirSync(${JSON.stringify(assets)})) fs.copyFileSync(path.join(${JSON.stringify(assets)}, f), path.join(out, f));
  process.exit(0);
}
if (sub === "edit") process.exit(0);
process.exit(1);
`);
  chmodSync(gh, 0o755);
  return { runDir, gh, callLog, notesCopy };
}

const publishArgs = (runDir, gh, extra = {}) => ({
  runDir, approvePublish: true, ghCommand: gh, env: { FERRYX_APPROVE_PUBLISH: "1" },
  lsRemoteFn: () => `${COMMIT}\trefs/tags/v2026.1024.1\n`, ...extra,
});

test("drafting refuses a FAIL fixture before any gh call", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-survival-publish-"));
  try {
    const { runDir, gh, callLog } = assembledRun(tmp);
    writeSurvivalFixture(runDir, COMMIT, { mutate: (s, p) => { if (p === "windows") s.scenarios.find((x) => x.id === "Q01").status = "FAIL"; } });
    await assert.rejects(() => publishRelease(publishArgs(runDir, gh)), (error) => error.code === "SURVIVAL_FAIL");
    assert.equal(existsSync(callLog), false);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("drafting with a waiver puts the report and the waiver in the release body", async () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-survival-publish-"));
  try {
    const { runDir, gh, notesCopy } = assembledRun(tmp);
    writeSurvivalFixture(runDir, COMMIT, { footprint: false });
    const result = await publishRelease(publishArgs(runDir, gh, { survivalWaiver: "footprint pending todo 17" }));
    assert.equal(result.published, true);
    assert.equal(result.survivalWaiver, "footprint pending todo 17");
    const body = readFileSync(notesCopy, "utf8");
    assert.match(body, /^# Ferryx release v2026\.1024\.1\n\n## Session survival\n/);
    assert.match(body, /\| Q08b \| PASS \(2\/2 sessions\) \| PASS \(2\/2 sessions\) \| PASS \(2\/2 sessions\) \|/);
    assert.match(body, /\*\*Survival waiver \(owner override\):\*\* footprint pending todo 17/);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});

test("CLI survival-gate exits non-zero on a FAIL fixture and zero on all PASS", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-survival-cli-"));
  try {
    const { runDir } = assembledRun(tmp);
    const script = join(process.cwd(), "scripts", "release-local.mjs");
    writeSurvivalFixture(runDir, COMMIT, { mutate: (s, p) => { if (p === "mac") s.scenarios.find((x) => x.id === "Q02").status = "FAIL"; } });
    const bad = spawnSync("bun", [script, "survival-gate", "--run", runDir], { encoding: "utf8", env: { ...process.env, CI: "", GITHUB_ACTIONS: "" } });
    assert.equal(bad.status, 1);
    assert.match(bad.stderr, /SURVIVAL_FAIL: /);
    rmSync(join(runDir, "survival"), { recursive: true });
    writeSurvivalFixture(runDir, COMMIT);
    const good = spawnSync("bun", [script, "survival-gate", "--run", runDir], { encoding: "utf8" });
    assert.equal(good.status, 0, good.stderr);
    assert.match(good.stdout, /Session survival gate passed/);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});
