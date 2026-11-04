#!/usr/bin/env bun
// Fails the job on a false check in a browser QA summary.json.
//
// Usage: bun scripts/browser-qa/check-summary.mjs <dir> [<dir>...]
//
// Reads every summary.json under the given dirs. A failure is a `false` entry of any `checks`
// object outside `steps` (per-step action records are evidence, not verdicts), unless the object
// that holds `checks` lists the name in its `informational` array, and a top-level `pass: false`.
// A dir without a summary.json fails too. Prints one line per check and exits 1 on any failure.
// A full UI-audit summary must also have run its engine and extension sections: an engine
// marked `skipped` or a missing `extension` fails, so a dropped engine bundle cannot pass.
// A CDP-audit summary (scenario "cdp", gha-cdp-audit.mjs) must carry every check of
// CDP_REQUIRED as `true`; a missing section or check fails, and `informational` cannot excuse one.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

/** The checks a CDP-audit summary must report true, per section; per-OS extras below. */
export const CDP_REQUIRED = {
  agent_cards: ["pair_card_shown", "pair_approved_by_click", "grant_card_shown", "grant_approved_by_click", "pipe_list", "pipe_observe_refs", "pipe_act_applied", "mcp_initialize", "mcp_tools_list", "mcp_snapshot_refs", "mcp_act_applied", "capture_webview", "capture_chromium", "app_alive_after_capture"],
  passkeys: ["cdp_connected", "virtual_authenticator_added", "create_resolved", "get_resolved", "one_credential_listed"],
  media: ["prompt_shown", "block_denies", "block_remembered", "reset_clears", "prompt_after_reset", "allow_grants", "allow_remembered", "prompt_after_second_reset"],
  download: ["download_started", "paused_by_click", "size_stable_while_paused", "resumed_by_click", "completed", "sha256_matches"],
  import: ["chrome_profile_seeded", "source_detected", "bookmarks_imported", "history_imported", "cookies_imported", "passwords_imported", "bitwarden_json_imported", "bitwarden_csv_imported"],
};
const CDP_REQUIRED_BY_PLATFORM = {
  linux: { download: ["disk_full_reported"] },
  win32: { import: ["v20_cookie_seen", "v20_cookie_decrypted"] },
};

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

/** Coverage failures of a full UI-audit summary: the sections that must have run. */
export function missingCoverage(summary) {
  if (summary?.scenario === "cdp") return missingCdpChecks(summary);
  if (summary?.scenario !== "full") return [];
  const missing = [];
  if (!summary.engine || summary.engine.skipped) missing.push(`engine skipped: ${summary.engine?.skipped ?? "no engine section"}`);
  if (!summary.extension) missing.push("extension scenario did not run (extension is null)");
  return missing;
}

/** Every required CDP check that is absent or not exactly `true`. */
export function missingCdpChecks(summary) {
  const extra = CDP_REQUIRED_BY_PLATFORM[summary.platform] ?? {};
  const missing = [];
  for (const [section, names] of Object.entries(CDP_REQUIRED)) {
    const checks = summary[section]?.checks;
    if (!checks || typeof checks !== "object") {
      missing.push(`${section}: section missing`);
      continue;
    }
    for (const name of [...names, ...(extra[section] ?? [])]) {
      if (!(name in checks)) missing.push(`${section}.${name} missing`);
      else if (checks[name] !== true) missing.push(`${section}.${name} = ${JSON.stringify(checks[name])}`);
    }
  }
  return missing;
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
      for (const reason of missingCoverage(summary)) {
        console.log(`FAIL ${name}: ${reason}`);
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
