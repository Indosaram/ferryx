import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  KEEPER_RETAINED_GENERATIONS,
  compareKeeperVersions,
  isKeeperVersion,
  keeperManifest,
  parseKeeperLedger,
  recordKeeperRelease,
} from "./lib/release-keeper.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const LEDGER_PATH = join(REPO_ROOT, "packaging", "keeper", "releases.json");

test("the checked-in ledger is valid and starts with no keeper release", () => {
  const ledger = parseKeeperLedger(readFileSync(LEDGER_PATH, "utf8"));
  assert.equal(ledger.schemaVersion, 1);
  assert.deepEqual(ledger.keeperReleases, [], "no release has shipped a keeper yet");
});

test("versions compare numerically and only three-component names are generations", () => {
  assert.equal(compareKeeperVersions("2026.10.9", "2026.9.10"), 1);
  assert.equal(compareKeeperVersions("2026.908.1", "2026.908.1"), 0);
  assert.equal(compareKeeperVersions("2026.908.1", "2026.908.2"), -1);
  assert.equal(compareKeeperVersions("1.2.3-beta.1", "1.2.3"), 1);

  assert.equal(isKeeperVersion("2026.1115.1"), true);
  assert.equal(isKeeperVersion("1.2.3-beta.1"), true);
  assert.equal(isKeeperVersion("keeper"), false);
  assert.equal(isKeeperVersion("2026.1115"), false);
  assert.equal(isKeeperVersion(""), false);
});

test("the first keeper release publishes itself as the oldest safe rollback version", () => {
  const ledger = { schemaVersion: 1, keeperReleases: [] };
  assert.deepEqual(keeperManifest({ appVersion: "2026.1116.1", ledger }), {
    oldestSafeRollbackVersion: "2026.1116.1",
    protocolMajor: 1,
    retainedGenerations: KEEPER_RETAINED_GENERATIONS,
  });
  assert.throws(
    () => keeperManifest({ appVersion: "v2026.1116.1", ledger }),
    /three-component appVersion/,
    "a tag is not an app version",
  );
});

test("every later release keeps the first keeper release as its floor", () => {
  const empty = { schemaVersion: 1, keeperReleases: [] };
  const ledger = recordKeeperRelease(empty, { appVersion: "2026.1116.1" });
  assert.deepEqual(ledger.keeperReleases, [
    { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
  ]);

  assert.equal(
    keeperManifest({ appVersion: "2026.1117.1", ledger }).oldestSafeRollbackVersion,
    "2026.1116.1",
    "a later release inherits the first keeper release",
  );

  const twoReleases = recordKeeperRelease(ledger, { appVersion: "2026.1117.1" });
  assert.equal(twoReleases.keeperReleases.length, 2);
  assert.equal(twoReleases.keeperReleases[1].oldestSafeRollbackVersion, "2026.1116.1");
  assert.equal(
    keeperManifest({ appVersion: "2026.1118.1", ledger: twoReleases }).oldestSafeRollbackVersion,
    "2026.1116.1",
    "the floor stays at the first keeper release, not at the newest one",
  );
});

test("re-deriving a recorded release reproduces the manifest it published", () => {
  const empty = { schemaVersion: 1, keeperReleases: [] };
  const published = keeperManifest({ appVersion: "2026.1116.1", ledger: empty });
  const recorded = recordKeeperRelease(empty, { appVersion: "2026.1116.1" });
  assert.deepEqual(
    keeperManifest({ appVersion: "2026.1116.1", ledger: recorded }),
    published,
    "assembling again after the ledger recorded the release must emit identical bytes",
  );

  const next = keeperManifest({ appVersion: "2026.1117.1", ledger: recorded });
  const nextRecorded = recordKeeperRelease(recorded, { appVersion: "2026.1117.1" });
  assert.deepEqual(keeperManifest({ appVersion: "2026.1117.1", ledger: nextRecorded }), next);
});

test("a ledger that is never appended publishes each release as its own floor", () => {
  const empty = { schemaVersion: 1, keeperReleases: [] };
  assert.equal(
    keeperManifest({ appVersion: "2026.1118.1", ledger: empty }).oldestSafeRollbackVersion,
    "2026.1118.1",
    "with no recorded keeper release, nothing older can be claimed as serviceable",
  );
  const recorded = recordKeeperRelease(empty, { appVersion: "2026.1116.1" });
  assert.equal(
    keeperManifest({ appVersion: "2026.1118.1", ledger: recorded }).oldestSafeRollbackVersion,
    "2026.1116.1",
    "recording the first keeper release restores the real floor",
  );
});

test("a recorded release is authoritative and an older unrecorded build is refused", () => {
  const empty = { schemaVersion: 1, keeperReleases: [] };
  const recorded = recordKeeperRelease(empty, { appVersion: "2026.1116.1" });
  assert.equal(
    keeperManifest({ appVersion: "2026.1116.1", ledger: recorded }).oldestSafeRollbackVersion,
    "2026.1116.1",
    "re-cutting the same recorded release publishes the floor it already published",
  );
  assert.throws(
    () => keeperManifest({ appVersion: "2026.1115.1", ledger: recorded }),
    /not newer than the latest recorded keeper release/,
    "a build older than the recorded keeper release cannot claim a floor it never had",
  );
  assert.throws(
    () => keeperManifest({ appVersion: "v2026.1115.1", ledger: empty }),
    /three-component appVersion/,
    "a malformed version is rejected before any floor is derived",
  );
  assert.deepEqual(
    recordKeeperRelease(recorded, { appVersion: "2026.1116.1" }),
    recorded,
    "recording an already recorded release is a no-op",
  );
});

test("the ledger rejects a floor that moves backwards or names a newer version", () => {
  assert.equal(
    parseKeeperLedger({
      schemaVersion: 1,
      keeperReleases: [
        { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
      ],
    }).keeperReleases.length,
    1,
    "a single well-formed entry is accepted",
  );

  assert.throws(
    () =>
      parseKeeperLedger({
        schemaVersion: 1,
        keeperReleases: [
          { appVersion: "2026.1116.1", protocolMajor: 2, oldestSafeRollbackVersion: "2026.1116.1" },
        ],
      }),
    /speaks protocol major 2/,
  );

  assert.throws(
    () =>
      parseKeeperLedger({
        schemaVersion: 1,
        keeperReleases: [
          { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1117.1" },
        ],
      }),
    /cannot roll back to a newer version/,
  );

  assert.throws(
    () =>
      parseKeeperLedger({
        schemaVersion: 1,
        keeperReleases: [
          { appVersion: "2026.1117.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1117.1" },
          { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
        ],
      }),
    /strictly increasing/,
  );

  assert.throws(
    () =>
      parseKeeperLedger({
        schemaVersion: 1,
        keeperReleases: [
          { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
          { appVersion: "2026.1117.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1115.1" },
        ],
      }),
    /floor may not move backwards/,
  );

  assert.throws(
    () => parseKeeperLedger({ schemaVersion: 1, keeperReleases: [], extra: true }),
    /unexpected property/,
  );
  assert.throws(
    () => parseKeeperLedger({ schemaVersion: 2, keeperReleases: [] }),
    /schemaVersion must be 1/,
  );
});
