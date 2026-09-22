import test from "node:test";
import assert from "node:assert/strict";

import {
  decideInstallStrategy,
  evaluateNotarizationEvidence,
  liveExecutorCommand,
  timestampedBackupDir,
  UNSIGNED_OVERRIDE_ENV,
} from "./install-macos-app.mjs";

const NOTARIZED_DV = [
  "Executable=/Applications/Ferryx.app/Contents/MacOS/ferryx",
  "Identifier=com.ferryx.app",
  "Authority=Developer ID Application: Indo Yoon (5DUM8WPB4C)",
  "TeamIdentifier=5DUM8WPB4C",
  "Sealed Resources=verified",
  "Signature=authenticated",
].join("\n");

test("evaluateNotarizationEvidence accepts a Developer ID notarized bundle", () => {
  const verdict = evaluateNotarizationEvidence({
    codesignDv: NOTARIZED_DV,
    spctl: "/Applications/Ferryx.app: accepted\nsource=Notarized Developer ID",
    staplerExit: 0,
  });
  assert.deepEqual(verdict.failures, []);
  assert.equal(verdict.ok, true);
});

test("evaluateNotarizationEvidence rejects the ad-hoc dev bundle that was installed on 2026-09-22", () => {
  const adhocDv = [
    "Executable=/Applications/Ferryx.app/Contents/MacOS/ferryx",
    "Identifier=ferryx-191234bf27b8b965",
    "CodeDirectory v=20400 size=270712 flags=0x20002(adhoc,linker-signed)",
    "Signature=adhoc",
    "TeamIdentifier=not set",
    "Sealed Resources=none",
  ].join("\n");
  const verdict = evaluateNotarizationEvidence({
    codesignDv: adhocDv,
    spctl: "/Applications/Ferryx.app: rejected\nsource=no usable signature",
    staplerExit: 65,
  });
  assert.equal(verdict.ok, false);
  assert.ok(verdict.failures.some((f) => f.includes("ad-hoc/linker signed")));
  assert.ok(verdict.failures.some((f) => f.includes("Developer ID")));
  assert.ok(verdict.failures.some((f) => f.includes("TeamIdentifier")));
  assert.ok(verdict.failures.some((f) => f.includes("Notarized Developer ID")));
  assert.ok(verdict.failures.some((f) => f.includes("stapler")));
});

test("evaluateNotarizationEvidence rejects a Developer ID bundle without a staple", () => {
  const verdict = evaluateNotarizationEvidence({
    codesignDv: NOTARIZED_DV,
    spctl: "/Applications/Ferryx.app: accepted\nsource=Notarized Developer ID",
    staplerExit: 69,
  });
  assert.equal(verdict.ok, false);
  assert.deepEqual(verdict.failures.length, 1);
  assert.ok(verdict.failures[0].includes("stapler"));
});

test("decideInstallStrategy always backs up instead of deleting while processes run", () => {
  const withLive = decideInstallStrategy({ liveExecutors: [27861, 72874] });
  assert.equal(withLive.action, "backup-then-replace");
  assert.ok(withLive.reasons.some((r) => r.includes("27861")));
  assert.ok(withLive.reasons.some((r) => r.includes("rename")));

  const withoutLive = decideInstallStrategy({ liveExecutors: [] });
  assert.equal(withoutLive.action, "backup-then-replace");
  assert.deepEqual(withoutLive.reasons, []);
});

test("decideInstallStrategy records the unsigned override", () => {
  const override = decideInstallStrategy({ liveExecutors: [], unsignedOverride: true });
  assert.ok(override.reasons.some((r) => r.includes("unsigned install override")));
});

test("liveExecutorCommand shells out to pgrep plus per-pid lsof txt matching", () => {
  const command = liveExecutorCommand("/Applications/Ferryx.app");
  assert.equal(command[0], "/bin/sh");
  assert.match(command[2], /pgrep -f '\/Applications\/Ferryx\.app\/Contents\/MacOS\/ferryx'/);
  assert.match(command[2], /awk '\$4=="txt"/);
  assert.match(command[2], /case "\$path" in '\/Applications\/Ferryx\.app'\/\*\)/);
});

test("timestampedBackupDir is inode-safe and timestamped under /Applications", () => {
  const dir = timestampedBackupDir(new Date("2026-09-22T13:10:21Z"));
  assert.match(dir, /^\/Applications\/\.ferryx-previous-\d{8}-\d{6}$/);
  assert.ok(!dir.endsWith("/Ferryx.app"));
});

test("unsigned override requires both the env var and the CLI flag together", () => {
  assert.equal(UNSIGNED_OVERRIDE_ENV, "FERRYX_ALLOW_UNSIGNED_INSTALL");
});
