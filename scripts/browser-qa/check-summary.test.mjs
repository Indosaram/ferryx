import { describe, expect, it } from "bun:test";

import { collectChecks, missingCoverage } from "./check-summary.mjs";

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
