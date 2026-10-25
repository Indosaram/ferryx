// Fixture writer for the release session survival gate (scripts/lib/release-survival.mjs).
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { newSummary } from "../qa/daemon-lifetime/lib/summary.mjs";
import { SURVIVAL_PLATFORMS, survivalScenariosFor } from "../lib/release-survival.mjs";

const HOSTS = { mac: "macbook", linux: "omarchy", windows: "maho-win" };

/** A daemon-lifetime summary for `platform` at `commit` with every gate scenario PASS. */
export function passingSummary(platform, commit) {
  const summary = newSummary({ runId: "20261024t120000z", platform, host: HOSTS[platform], revisions: { head: commit } });
  for (const s of summary.scenarios) {
    if (s.applicable && (s.id === "Q00" || survivalScenariosFor(platform).includes(s.id))) {
      s.status = "PASS";
      s.assertions = [{ name: "fixture", ok: true, detail: null }];
      s.evidence = ["fixture.log"];
      s.sessions = { before: 2, kept: 2 };
    }
  }
  summary.exitCode = 0;
  return summary;
}

/**
 * Writes survival/<platform>/summary.json for every platform (all PASS) and survival/footprint.json.
 * `mutate(summary, platform)` edits a summary before it is written; `platforms` and `footprint`
 * leave pieces out.
 */
export function writeSurvivalFixture(runDir, commit, { platforms = SURVIVAL_PLATFORMS, footprint = true, mutate = () => {} } = {}) {
  const dir = join(runDir, "survival");
  for (const platform of platforms) {
    const summary = passingSummary(platform, commit);
    mutate(summary, platform);
    mkdirSync(join(dir, platform), { recursive: true });
    writeFileSync(join(dir, platform, "summary.json"), `${JSON.stringify(summary, null, 2)}\n`);
  }
  mkdirSync(dir, { recursive: true });
  if (footprint) writeFileSync(join(dir, "footprint.json"), `${JSON.stringify({ schema: "fixture", sessions: [1, 10, 30] })}\n`);
  return dir;
}
