import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { assertReleaseTag, checkReleaseTag } from "./release-tag-policy.mjs";
import { parseReleaseTag, toAppVersion, toMsixVersion } from "./sync-version.mjs";

const SCRIPTS = import.meta.dir;

test("accepts vYYYY.MMDD.N tags", () => {
  expect(assertReleaseTag("v2026.1024.1")).toEqual({ year: 2026, month: 10, day: 24, revision: 1 });
  expect(checkReleaseTag("v2026.0908.12").ok).toBe(true);
  expect(checkReleaseTag("v2028.0229.1").ok).toBe(true);
});

test("rejects legacy and malformed tags", () => {
  for (const tag of [
    "0.1.0",
    "v0.1.0",
    "v2026.08.26.1",
    "v2026.10.11.1",
    "2026.1024.1",
    "v2026.1024",
    "v2026.1024.0",
    "v2026.1024.01",
    "v2026.1324.1",
    "v2026.0229.1",
    "v2026.1031.1x",
    "v2025.1231.1",
    "v2026.1024.65536",
    "",
  ]) {
    expect(checkReleaseTag(tag).ok).toBe(false);
    expect(() => assertReleaseTag(tag)).toThrow(/Invalid release tag/);
  }
});

test("a conforming tag maps to app version YYYY.(MM*100+DD).N", () => {
  expect(parseReleaseTag("v2026.1024.1")).toEqual({ year: 2026, month: 10, day: 24, revision: 1 });
  expect(toAppVersion("v2026.1024.1")).toBe("2026.1024.1");
  expect(toAppVersion("v2026.0908.1")).toBe("2026.908.1");
  expect(toMsixVersion("v2026.0908.1")).toBe("2026.908.1.0");
  expect(() => parseReleaseTag("v2026.0230.1")).toThrow(/calendar date/);
});

test("CLI exits non-zero for a malformed tag and zero for a conforming one", () => {
  const bad = spawnSync("bun", [join(SCRIPTS, "release-tag-policy.mjs"), "0.1.0"], { encoding: "utf8" });
  expect(bad.status).toBe(1);
  expect(bad.stderr).toMatch(/does not match vYYYY\.MMDD\.N/);
  const good = spawnSync("bun", [join(SCRIPTS, "release-tag-policy.mjs"), "v2026.1024.1"], { encoding: "utf8" });
  expect(good.status).toBe(0);
});

test("release-local prepare rejects a malformed tag before reading config or building", () => {
  const tmp = mkdtempSync(join(tmpdir(), "ferryx-tag-policy-"));
  try {
    const res = spawnSync(
      "bun",
      [
        join(SCRIPTS, "release-local.mjs"),
        "prepare",
        "--config",
        join(tmp, "missing-config.json"),
        "--tag",
        "v2026.08.26.1",
        "--commit",
        "HEAD",
        "--out",
        join(tmp, "run"),
      ],
      { encoding: "utf8", env: { ...process.env, CI: "", GITHUB_ACTIONS: "" } },
    );
    expect(res.status).not.toBe(0);
    // The policy error, not a missing-config error, proves the tag is checked first.
    expect(res.stderr + res.stdout).toMatch(/Invalid release tag: "v2026\.08\.26\.1" does not match vYYYY\.MMDD\.N/);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
});
