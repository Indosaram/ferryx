// Session survival release gate (runbook section 4b). A release is drafted only when every QA
// host produced a daemon-lifetime summary.json for the release commit with zero FAIL, and the
// footprint benchmark (bench/footprint, todo 17) left footprint.json next to them.
//
// Layout under the release run directory:
//   survival/<platform>/summary.json   node scripts/qa/daemon-lifetime/run.mjs --platform <p> --only ...
//   survival/footprint.json            bench/footprint/run.mjs
//   survival/survival-report.md        written by this gate
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { applicable } from "../qa/daemon-lifetime/lib/summary.mjs";

export const SURVIVAL_SCENARIOS = Object.freeze(["Q01", "Q02", "Q03", "Q03b", "Q07", "Q08", "Q08b", "Q11", "Q14"]);
export const SURVIVAL_PLATFORMS = Object.freeze(["mac", "linux", "windows"]);
export const SURVIVAL_REPORT_NAME = "survival-report.md";

/** Problem codes an owner waiver may cover: evidence that could not be produced (unreachable host). */
export const WAIVABLE_CODES = Object.freeze([
  "SURVIVAL_SUMMARY_MISSING",
  "SURVIVAL_SCENARIO_NOT_PASSED",
  "SURVIVAL_FOOTPRINT_MISSING",
]);

/** Typed refusal: `code` is the first blocking SURVIVAL_* code, `problems` lists every one. */
export class SurvivalGateError extends Error {
  constructor(code, message, problems = []) {
    super(`${code}: ${message}`);
    this.name = "SurvivalGateError";
    this.code = code;
    this.problems = problems;
  }
}

/** Gate scenario ids that summary.mjs lists for a platform (Q03b and Q08 are not run on windows). */
export function survivalScenariosFor(platform) {
  return SURVIVAL_SCENARIOS.filter((id) => applicable(id, platform));
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return null;
  }
}

/**
 * Evaluates the gate without throwing. rows[scenario][platform] is { status, sessions }, or null
 * where summary.mjs does not list the scenario for that platform.
 */
export function evaluateSurvivalGate({ releaseDir, commitSha, waiver = null }) {
  const survivalDir = join(releaseDir, "survival");
  const problems = [];
  const platforms = {};
  const rows = Object.fromEntries(SURVIVAL_SCENARIOS.map((id) => [id, {}]));

  for (const platform of SURVIVAL_PLATFORMS) {
    const path = join(survivalDir, platform, "summary.json");
    const wanted = survivalScenariosFor(platform);
    for (const id of SURVIVAL_SCENARIOS) rows[id][platform] = wanted.includes(id) ? { status: "MISSING", sessions: null } : null;
    if (!existsSync(path)) {
      platforms[platform] = { state: "missing", path };
      problems.push({ code: "SURVIVAL_SUMMARY_MISSING", platform, detail: `${path} does not exist` });
      continue;
    }
    const summary = readJson(path);
    if (!summary || !Array.isArray(summary.scenarios)) {
      platforms[platform] = { state: "invalid", path };
      problems.push({ code: "SURVIVAL_SUMMARY_INVALID", platform, detail: `${path} is not a daemon-lifetime summary` });
      continue;
    }
    platforms[platform] = { state: "present", path, host: summary.host ?? null, runId: summary.runId ?? null, commit: summary.commit ?? null };
    if (summary.platform !== platform) {
      problems.push({ code: "SURVIVAL_SUMMARY_INVALID", platform, detail: `${path}: platform is ${JSON.stringify(summary.platform)}` });
    }
    if (summary.commit !== commitSha) {
      problems.push({ code: "SURVIVAL_STALE_COMMIT", platform, detail: `${platform} summary commit ${JSON.stringify(summary.commit ?? null)} is not the release commit ${commitSha}` });
    }
    for (const id of wanted) {
      const scenario = summary.scenarios.find((s) => s.id === id);
      const status = scenario?.status ?? "MISSING";
      rows[id][platform] = { status, sessions: scenario?.sessions ?? null };
      if (status === "FAIL") {
        problems.push({ code: "SURVIVAL_FAIL", platform, scenario: id, detail: `${id} FAIL on ${platform}` });
      } else if (status !== "PASS") {
        problems.push({ code: "SURVIVAL_SCENARIO_NOT_PASSED", platform, scenario: id, detail: `${id} is ${status} on ${platform}` });
      }
    }
  }

  const footprintPath = join(survivalDir, "footprint.json");
  const footprint = existsSync(footprintPath) ? readJson(footprintPath) : null;
  if (!footprint) {
    problems.push({ code: "SURVIVAL_FOOTPRINT_MISSING", detail: `${footprintPath} is missing or not JSON (bench/footprint, todo 17)` });
  }

  const reason = typeof waiver === "string" && waiver.trim() ? waiver.trim() : null;
  // A waiver covers evidence that could not be produced; a FAIL, a stale commit or an invalid
  // summary always blocks.
  const blocking = problems.filter((p) => !(reason && WAIVABLE_CODES.includes(p.code)));
  return {
    ok: blocking.length === 0,
    commitSha,
    waiver: reason,
    rows,
    platforms,
    footprint: footprint ? { path: footprintPath, data: footprint } : null,
    problems,
    blocking,
  };
}

function cell(entry) {
  if (entry === null) return "n/a";
  const s = entry.sessions;
  if (s && Number.isInteger(s.before) && Number.isInteger(s.kept)) return `${entry.status} (${s.kept}/${s.before} sessions)`;
  return entry.status;
}

/** Markdown report: scenario x platform PASS/FAIL table with session counts, waiver and footprint. */
export function renderSurvivalReport(result) {
  const lines = [
    "## Session survival",
    "",
    `Release commit \`${result.commitSha}\`. Gate scenarios: ${SURVIVAL_SCENARIOS.join(", ")}. Cells read STATUS (kept/before live sessions).`,
    "",
    `| Scenario | ${SURVIVAL_PLATFORMS.join(" | ")} |`,
    `| --- | ${SURVIVAL_PLATFORMS.map(() => "---").join(" | ")} |`,
  ];
  for (const id of SURVIVAL_SCENARIOS) {
    lines.push(`| ${id} | ${SURVIVAL_PLATFORMS.map((p) => cell(result.rows[id][p])).join(" | ")} |`);
  }
  lines.push("", "| Platform | Host | Run | Summary commit |", "| --- | --- | --- | --- |");
  for (const p of SURVIVAL_PLATFORMS) {
    const info = result.platforms[p];
    lines.push(`| ${p} | ${info?.host ?? "-"} | ${info?.runId ?? info?.state ?? "-"} | ${info?.commit ?? "-"} |`);
  }
  lines.push("", `Footprint (footprint.json): ${result.footprint ? "present" : "MISSING"}.`);
  if (result.waiver) {
    lines.push("", `**Survival waiver (owner override):** ${result.waiver}`);
    const waived = result.problems.filter((p) => !result.blocking.includes(p));
    if (waived.length) lines.push("", "Waived findings:", "", ...waived.map((p) => `- ${p.code}: ${p.detail}`));
  }
  if (result.blocking.length) {
    lines.push("", "Blocking findings:", "", ...result.blocking.map((p) => `- ${p.code}: ${p.detail}`));
  }
  return `${lines.join("\n")}\n`;
}

/**
 * Runs the gate and writes survival/survival-report.md. Throws SurvivalGateError carrying the
 * first blocking code when the release must not be drafted.
 */
export function assertSurvivalGate({ releaseDir, commitSha, waiver = null }) {
  const result = evaluateSurvivalGate({ releaseDir, commitSha, waiver });
  const report = renderSurvivalReport(result);
  const survivalDir = join(releaseDir, "survival");
  mkdirSync(survivalDir, { recursive: true });
  const reportPath = join(survivalDir, SURVIVAL_REPORT_NAME);
  writeFileSync(reportPath, report);
  if (!result.ok) {
    throw new SurvivalGateError(
      result.blocking[0].code,
      `session survival gate refused the release (${result.blocking.map((p) => p.detail).join("; ")})`,
      result.blocking,
    );
  }
  return { result, report, reportPath };
}
