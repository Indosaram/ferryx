import { describe, expect, it, test } from "bun:test";

import { CDP_REQUIRED, appExitFailure, collectChecks, missingCdpChecks, missingCoverage, reportSummary } from "./check-summary.mjs";

const gaps = (names) => Object.fromEntries(names.map((name) => [name, { checks: { observed: true } }]));
const full = (platform, daily) => ({
  scenario: "full",
  platform,
  engine: { checks: { installed_from_settings: true } },
  extension: { checks: {} },
  daily,
});

describe("check-summary daily coverage", () => {
  it("fails a full summary that is missing a daily gap", () => {
    const daily = gaps(["g1", "g2", "g6", "g8", "g9", "g11", "g13"]);
    expect(missingCoverage(full("linux", daily))).toEqual(["daily g3 did not run"]);
  });

  it("does not require notification and print gaps on macOS", () => {
    expect(missingCoverage(full("darwin", gaps(["g1", "g2", "g3", "g8", "g11", "g13"])))).toEqual([]);
  });

  it("requires notification and print gaps on Windows", () => {
    expect(missingCoverage(full("win32", gaps(["g1", "g2", "g3", "g8", "g11", "g13"])))).toEqual([
      "daily g6 did not run",
      "daily g9 did not run",
    ]);
  });

  it("reports a false daily check as a failing row", () => {
    const rows = collectChecks({ daily: { g3: { checks: { saved: false, keystore_item: true } } } });
    expect(rows).toContainEqual({ path: "daily.g3.checks.saved", value: false, informational: false });
  });
});

function passing(platform) {
  const summary = { scenario: "cdp", platform };
  for (const [section, names] of Object.entries(CDP_REQUIRED)) summary[section] = { checks: Object.fromEntries(names.map((name) => [name, true])) };
  if (platform === "linux") summary.download.checks.disk_full_reported = true;
  if (platform === "win32") Object.assign(summary.import.checks, { v20_cookie_seen: true, v20_cookie_reported: true, v20_cookie_not_applied: true });
  else summary.import.checks.cookies_imported = true;
  return summary;
}

test("a cdp summary with every required check true passes on each platform", () => {
  for (const platform of ["linux", "darwin", "win32"]) expect(missingCoverage(passing(platform))).toEqual([]);
});

test("a false, missing or skipped cdp check fails", () => {
  const summary = passing("darwin");
  summary.passkeys.checks.get_resolved = false;
  delete summary.media.checks.allow_remembered;
  summary.download.checks.completed = "skipped";
  summary.media.informational = ["allow_remembered"];
  delete summary.import;
  expect(missingCdpChecks(summary)).toEqual([
    "passkeys.get_resolved = false",
    "media.allow_remembered missing",
    "download.completed = \"skipped\"",
    "import: section missing",
  ]);
});

test("per-platform checks are required only on their platform", () => {
  const linux = passing("linux");
  delete linux.download.checks.disk_full_reported;
  expect(missingCdpChecks(linux)).toEqual(["download.disk_full_reported missing"]);
  const windows = passing("win32");
  windows.import.checks.v20_cookie_reported = false;
  expect(missingCdpChecks(windows)).toEqual(["import.v20_cookie_reported = false"]);
  expect(missingCdpChecks(passing("darwin"))).toEqual([]);
});

test("cookies must import on linux and macos; windows asserts the v20 fallback instead", () => {
  for (const platform of ["linux", "darwin"]) {
    const summary = passing(platform);
    delete summary.import.checks.cookies_imported;
    expect(missingCdpChecks(summary)).toEqual(["import.cookies_imported missing"]);
  }
  const windows = passing("win32");
  delete windows.import.checks.v20_cookie_not_applied;
  expect(missingCdpChecks(windows)).toEqual(["import.v20_cookie_not_applied missing"]);
});

// Windows run 45014020714: the debug exe never loaded (0xC0000135 = exit 53) and the audit
// reported 48 skipped_app_exited steps plus 55 false checks instead of the launch failure.
const deadApp = (app_exit, totals = { ok: 0, skipped_app_exited: 48 }) => ({
  ...passing("win32"),
  binary: "/d/a/ferryx/src-tauri/target/debug/ferryx.exe",
  app_exit,
  totals,
});

test("a nonzero app exit outside teardown fails once, naming the binary and the code", () => {
  const summary = deadApp({ code: 53, signal: null, at: "2026-11-22T11:11:32.999Z", during_teardown: false });
  const report = reportSummary("ui-audit-windows", summary);
  expect(report.failures).toBe(1);
  expect(report.lines).toHaveLength(2);
  expect(report.lines[0].verdict).toBe("FAIL");
  expect(report.lines[0].text).toContain("exit code 53");
  expect(report.lines[0].text).toContain(summary.binary);
  expect(report.lines[1].text).toContain("48 step(s) recorded skipped_app_exited");
  // The skipped step check rows are gone: they were consequences of the exit, not verdicts.
  expect(report.lines.some((line) => line.text.includes(".checks."))).toBe(false);
});

test("an app exit with no during_teardown flag is still a failure", () => {
  expect(appExitFailure({ app_exit: { code: 1, signal: null } })).toContain("exit code 1");
  expect(appExitFailure({ app_exit: { code: null, signal: "SIGSEGV" } })).toContain("signal SIGSEGV");
});

test("a zero exit, a teardown stop and no exit at all are not failures", () => {
  expect(appExitFailure({ app_exit: { code: 0, signal: null, during_teardown: false } })).toBeNull();
  expect(appExitFailure({ app_exit: { code: null, signal: "SIGTERM", during_teardown: true } })).toBeNull();
  expect(appExitFailure({ app_exit: { code: 53, signal: null, during_teardown: true } })).toBeNull();
  expect(appExitFailure(passing("darwin"))).toBeNull();
});

test("a live app still reports its own failing checks", () => {
  const report = reportSummary("ui-audit-macos", { platform: "darwin", checks: { working: true, broken: false, noted: "x" }, informational: ["noted"] });
  expect(report.failures).toBe(1);
  expect(report.lines.filter((line) => line.verdict === "FAIL").map((line) => line.text)).toEqual(["ui-audit-macos: checks.broken = false"]);
});

test("the summary names the binary when it carries none", () => {
  expect(appExitFailure({ app_exit: { code: 53, signal: null } })).toContain("summary has no binary field");
});
