import { expect, test } from "bun:test";
import { CDP_REQUIRED, missingCdpChecks, missingCoverage } from "./check-summary.mjs";

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
