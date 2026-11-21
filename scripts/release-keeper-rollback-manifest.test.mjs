// Coverage for the bundled rollback floor (scripts/lib/keeper-rollback-manifest.mjs).
//
// The shipped promise: a macOS app bundle carries `Contents/Resources/keeper/latest.json` whose
// `keeper.oldestSafeRollbackVersion` is exactly what the checked-in ledger derives for that release,
// written only after a keeper payload of the same generation is in the bundle. Each case below
// breaks one fact and requires a typed refusal, because the alternative -- a bundle that ships a
// floor the release page does not state -- is a rollback that silently reaches an unsafe build.
//
// The ledger contract the ledger owner confirmed, and which these cases pin:
//   * exactly {schemaVersion, keeperReleases};
//   * each entry exactly {appVersion, protocolMajor, oldestSafeRollbackVersion};
//   * protocolMajor 1;
//   * appVersion strictly increasing, floor non-decreasing;
//   * the build requires only PRE-EXISTING entries to be committed -- never this release's own, so
//     publishing cannot become circular.
//
// No build, no codesign, no network, no clock: a fixture app bundle and a temporary ledger.
import test from "node:test";
import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { KEEPER_MANIFEST_NAME, KEEPER_MANIFEST_SCHEMA, KEEPER_RESOURCE_DIR } from "./lib/keeper-bundle.mjs";
import {
  KEEPER_ROLLBACK_MANIFEST_NAME,
  KeeperRollbackManifestError,
  assertFloorNotShadowed,
  assertKeeperRollbackManifest,
  assertKeeperFloorMatchesPublished,
  bundledUpdaterManifestPath,
  keeperRollbackManifest,
  keeperRollbackManifestPath,
  publishedFloor,
  writeKeeperRollbackManifest,
} from "./lib/keeper-rollback-manifest.mjs";
import { keeperManifest } from "./lib/release-keeper.mjs";

const EMPTY_LEDGER = { schemaVersion: 1, keeperReleases: [] };

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "keeper-rollback-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

/** A source tree shaped like the repository: `packaging/keeper/releases.json` under it. */
function writeSource(root, ledger) {
  const dir = join(root, "packaging", "keeper");
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "releases.json"), `${JSON.stringify(ledger, null, 2)}\n`);
  return root;
}

/** An app bundle carrying a keeper payload for `version`, as `installKeeperIntoApp` leaves it. */
function writeApp(root, version) {
  const appPath = join(root, "Ferryx.app");
  const resourceDir = join(appPath, "Contents", "Resources", KEEPER_RESOURCE_DIR);
  mkdirSync(resourceDir, { recursive: true });
  writeFileSync(
    join(resourceDir, KEEPER_MANIFEST_NAME),
    `${JSON.stringify({ schema: KEEPER_MANIFEST_SCHEMA, version, binary: "ferryx-ptykeeper" }, null, 2)}\n`,
  );
  return appPath;
}

function refusal(code, fn) {
  assert.throws(fn, (error) => {
    assert.ok(
      error instanceof KeeperRollbackManifestError,
      `expected KeeperRollbackManifestError, got ${error?.name}: ${error?.message}`,
    );
    assert.equal(error.code, code, `expected ${code}, got ${error.code}: ${error.message}`);
    return true;
  });
}

/** The committed ledger text a build sees, as `git show HEAD:` would return it. */
function committed(ledger) {
  return `${JSON.stringify(ledger, null, 2)}\n`;
}

/** The resource name and directory are a cross-language contract with `daemon::rollback`. */
test("the resource lives where daemon::rollback searches for it", (t) => {
  const root = fixture(t);
  const appPath = join(root, "Ferryx.app");
  assert.equal(
    keeperRollbackManifestPath(appPath),
    join(appPath, "Contents", "Resources", "keeper", "latest.json"),
    "src-tauri/src/daemon/rollback.rs::shipped_manifest_path reads Resources/keeper/latest.json",
  );
  assert.equal(KEEPER_ROLLBACK_MANIFEST_NAME, "latest.json");
  assert.equal(KEEPER_RESOURCE_DIR, "keeper");
});

test("an empty ledger publishes the release itself as the floor", (t) => {
  const root = fixture(t);
  const source = writeSource(root, EMPTY_LEDGER);
  const document = keeperRollbackManifest({
    appVersion: "2026.1116.1",
    ledgerPath: join(source, "packaging", "keeper", "releases.json"),
    committedLedgerText: committed(EMPTY_LEDGER),
  });
  // The one derivation is `keeperManifest`; assert this module agrees with it rather than with a
  // floor literal of its own.
  assert.equal(
    keeperManifest({ appVersion: "2026.1116.1", ledger: EMPTY_LEDGER }).oldestSafeRollbackVersion,
    "2026.1116.1",
  );
  assert.equal(document.keeper.oldestSafeRollbackVersion, "2026.1116.1");
  assert.equal(document.version, "2026.1116.1");
});

test("the floor is the ledger's oldest recorded release, and a released version keeps its own", (t) => {
  const root = fixture(t);
  const ledger = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
      { appVersion: "2026.1117.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };
  const source = writeSource(root, ledger);
  const ledgerPath = join(source, "packaging", "keeper", "releases.json");

  // A release newer than every entry inherits the oldest floor: the first keeper-capable build.
  assert.deepEqual(keeperRollbackManifest({ appVersion: "2026.1118.1", ledgerPath, committedLedgerText: committed(ledger) }), {
    version: "2026.1118.1",
    keeper: { oldestSafeRollbackVersion: "2026.1116.1", protocolMajor: 1, retainedGenerations: 3 },
  });
  // A recorded release replays its own entry, so re-assembling release N is byte-comparable.
  assert.equal(
    keeperRollbackManifest({ appVersion: "2026.1117.1", ledgerPath, committedLedgerText: committed(ledger) })
      .keeper.oldestSafeRollbackVersion,
    "2026.1116.1",
  );
});

test("writing plants the floor beside a matching payload and asserting accepts it", (t) => {
  const root = fixture(t);
  const source = writeSource(root, EMPTY_LEDGER);
  const appPath = writeApp(root, "2026.1116.1");

  const written = writeKeeperRollbackManifest({
    appPath,
    appVersion: "2026.1116.1",
    sourceDir: source,
    committedLedgerText: committed(EMPTY_LEDGER),
  });
  assert.equal(written.manifestPath, keeperRollbackManifestPath(appPath));
  assert.equal(written.document.keeper.oldestSafeRollbackVersion, "2026.1116.1");
  assert.ok(existsSync(written.manifestPath));

  const checked = assertKeeperRollbackManifest({
    appPath,
    appVersion: "2026.1116.1",
    sourceDir: source,
    committedLedgerText: committed(EMPTY_LEDGER),
  });
  assert.deepEqual(checked.manifest, written.document);
});

test("a missing ledger is a refusal, never an empty floor", (t) => {
  const root = fixture(t);
  const source = join(root, "no-ledger");
  mkdirSync(source, { recursive: true });
  refusal("KEEPER_LEDGER_MISSING", () =>
    keeperRollbackManifest({ appVersion: "2026.1116.1", ledgerPath: join(source, "packaging", "keeper", "releases.json") }),
  );
  // The same refusal through the write path, before any file is planted.
  const appPath = writeApp(root, "2026.1116.1");
  refusal("KEEPER_LEDGER_MISSING", () =>
    writeKeeperRollbackManifest({ appPath, appVersion: "2026.1116.1", sourceDir: source }),
  );
  assert.equal(existsSync(keeperRollbackManifestPath(appPath)), false, "a refusal writes no manifest");
});

test("an untrusted ledger is refused rather than read as a floor", (t) => {
  const root = fixture(t);
  const appPath = writeApp(mkdirRecursive(root, "app-root"), "2026.1116.1");

  const cases = {
    // A key the packager does not know about: the document is not the ledger it thinks it is.
    "unexpected key": { schemaVersion: 1, keeperReleases: [], extra: true },
    // A floor that moves backwards between releases lets a rollback reach a keeper-less build.
    "floor moves backwards": {
      schemaVersion: 1,
      keeperReleases: [
        { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
        { appVersion: "2026.1117.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1115.1" },
      ],
    },
    // A protocol the packager does not speak cannot be judged compatible at install time.
    "protocol major drift": {
      schemaVersion: 1,
      keeperReleases: [{ appVersion: "2026.1116.1", protocolMajor: 2, oldestSafeRollbackVersion: "2026.1116.1" }],
    },
    // A release recorded out of order makes "the oldest entry" meaningless.
    "out of order": {
      schemaVersion: 1,
      keeperReleases: [
        { appVersion: "2026.1117.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
        { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
      ],
    },
  };
  for (const [name, ledger] of Object.entries(cases)) {
    const dir = mkdirRecursive(root, name.replace(/\s+/g, "-"));
    const source = writeSource(dir, ledger);
    refusal("KEEPER_LEDGER_UNTRUSTED", () =>
      writeKeeperRollbackManifest({
        appPath,
        appVersion: "2026.1118.1",
        sourceDir: source,
        committedLedgerText: committed(ledger),
      }),
    );
  }

  // Unparseable JSON is the same refusal, not a crash.
  const broken = mkdirRecursive(root, "broken-json");
  mkdirSync(join(broken, "packaging", "keeper"), { recursive: true });
  writeFileSync(join(broken, "packaging", "keeper", "releases.json"), "{ not json");
  refusal("KEEPER_LEDGER_UNTRUSTED", () =>
    writeKeeperRollbackManifest({
      appPath,
      appVersion: "2026.1118.1",
      sourceDir: broken,
      committedLedgerText: committed(EMPTY_LEDGER),
    }),
  );
  assert.equal(existsSync(keeperRollbackManifestPath(appPath)), false, "no manifest from an untrusted ledger");
});

test("a floor above the release being built is refused", (t) => {
  const root = fixture(t);
  const ledger = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1117.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };
  const source = writeSource(root, ledger);
  const appPath = writeApp(root, "2026.1117.1");
  // 2026.1116.1 is already recorded, so an older build cannot be cut; the floor it would publish
  // is above itself. `keeperManifest` refuses this first, and the module restates it.
  refusal("KEEPER_LEDGER_UNTRUSTED", () =>
    keeperRollbackManifest({
      appVersion: "2026.1116.1",
      ledgerPath: join(source, "packaging", "keeper", "releases.json"),
      committedLedgerText: committed(ledger),
    }),
  );
  refusal("KEEPER_LEDGER_UNTRUSTED", () =>
    writeKeeperRollbackManifest({
      appPath,
      appVersion: "2026.1116.1",
      sourceDir: source,
      committedLedgerText: committed(ledger),
    }),
  );
});

test("no keeper payload means no floor is planted", (t) => {
  const root = fixture(t);
  const source = writeSource(root, EMPTY_LEDGER);
  const appPath = join(root, "Ferryx.app");
  mkdirSync(join(appPath, "Contents", "Resources"), { recursive: true });
  refusal("KEEPER_PAYLOAD_ABSENT", () =>
    writeKeeperRollbackManifest({
      appPath,
      appVersion: "2026.1116.1",
      sourceDir: source,
      committedLedgerText: committed(EMPTY_LEDGER),
    }),
  );
});

test("a payload of another generation is refused: the floor would describe a keeper that is not there", (t) => {
  const root = fixture(t);
  const source = writeSource(root, EMPTY_LEDGER);
  const appPath = writeApp(root, "2026.1115.1");
  refusal("KEEPER_PAYLOAD_VERSION_MISMATCH", () =>
    writeKeeperRollbackManifest({
      appPath,
      appVersion: "2026.1116.1",
      sourceDir: source,
      committedLedgerText: committed(EMPTY_LEDGER),
    }),
  );
});

test("asserting names a missing, unreadable or drifted manifest", (t) => {
  const root = fixture(t);
  const ledger = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };
  const source = writeSource(root, ledger);
  const appPath = writeApp(root, "2026.1116.1");
  const seam = { appPath, appVersion: "2026.1116.1", sourceDir: source, committedLedgerText: committed(ledger) };

  // The state this gate exists to make impossible: a bundle with a keeper but no floor.
  refusal("KEEPER_ROLLBACK_MANIFEST_MISSING", () => assertKeeperRollbackManifest(seam));

  writeKeeperRollbackManifest(seam);
  const manifestPath = keeperRollbackManifestPath(appPath);

  // A floor that is merely *valid* is not enough: it must equal what the ledger derives.
  const drifted = JSON.parse(readFileSync(manifestPath, "utf8"));
  drifted.keeper.oldestSafeRollbackVersion = "2026.1115.1";
  writeFileSync(manifestPath, `${JSON.stringify(drifted, null, 2)}\n`);
  refusal("KEEPER_ROLLBACK_MANIFEST_MISMATCH", () => assertKeeperRollbackManifest(seam));

  // An extra key is drift too: the document must be exactly the derived one.
  const extra = JSON.parse(readFileSync(manifestPath, "utf8"));
  extra.platforms = { "darwin-aarch64": { url: "https://example.invalid/x" } };
  writeFileSync(manifestPath, `${JSON.stringify(extra, null, 2)}\n`);
  refusal("KEEPER_ROLLBACK_MANIFEST_MISMATCH", () => assertKeeperRollbackManifest(seam));

  writeFileSync(manifestPath, "{ not json");
  refusal("KEEPER_ROLLBACK_MANIFEST_UNREADABLE", () => assertKeeperRollbackManifest(seam));
});

test("the bundled document parses the way daemon::rollback reads it", (t) => {
  const root = fixture(t);
  const ledger = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
      { appVersion: "2026.1117.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };
  const source = writeSource(root, ledger);
  const appPath = writeApp(root, "2026.1118.1");
  writeKeeperRollbackManifest({
    appPath,
    appVersion: "2026.1118.1",
    sourceDir: source,
    committedLedgerText: committed(ledger),
  });
  const parsed = JSON.parse(readFileSync(keeperRollbackManifestPath(appPath), "utf8"));

  // `floor_from_manifest` reads `keeper.oldestSafeRollbackVersion` as a string and nothing else.
  assert.equal(typeof parsed.keeper.oldestSafeRollbackVersion, "string");
  assert.equal(parsed.keeper.oldestSafeRollbackVersion, "2026.1116.1");
  // It is NOT an updater manifest: no `platforms` map, so nothing mistakes it for one.
  assert.equal(parsed.platforms, undefined);
  assert.deepEqual(Object.keys(parsed).sort(), ["keeper", "version"]);
});

function mkdirRecursive(root, name) {
  const dir = join(root, name);
  mkdirSync(dir, { recursive: true });
  return dir;
}

// ---------------------------------------------------------------------------------------------
// The ledger owner's contract: only PRE-EXISTING entries need to be committed, never this release.
// ---------------------------------------------------------------------------------------------

test("this release's own entry need not be committed: the build is not circular", (t) => {
  const root = fixture(t);
  const prior = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };
  // The working tree has already appended this release; HEAD has not. A publish that appends before
  // committing must still be buildable, otherwise the release cannot be cut at all.
  const source = writeSource(root, {
    schemaVersion: 1,
    keeperReleases: [
      ...prior.keeperReleases,
      { appVersion: "2026.1117.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  });
  const appPath = writeApp(root, "2026.1117.1");
  const written = writeKeeperRollbackManifest({
    appPath,
    appVersion: "2026.1117.1",
    sourceDir: source,
    committedLedgerText: committed(prior),
  });
  assert.equal(written.document.keeper.oldestSafeRollbackVersion, "2026.1116.1");
});

test("a working tree with no append at all is equally buildable", (t) => {
  const root = fixture(t);
  const prior = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };
  const source = writeSource(root, prior);
  const appPath = writeApp(root, "2026.1117.1");
  const written = writeKeeperRollbackManifest({
    appPath,
    appVersion: "2026.1117.1",
    sourceDir: source,
    committedLedgerText: committed(prior),
  });
  assert.equal(written.document.keeper.oldestSafeRollbackVersion, "2026.1116.1");
});

test("the first keeper release may introduce an uncommitted ledger, but may not inherit history", (t) => {
  const root = fixture(t);
  // No committed ledger, working tree records only this release: the documented first-release case.
  const first = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };
  const source = writeSource(root, first);
  const appPath = writeApp(root, "2026.1116.1");
  assert.equal(
    keeperRollbackManifest({
      appVersion: "2026.1116.1",
      ledgerPath: join(source, "packaging", "keeper", "releases.json"),
      committedLedgerText: null,
    }).keeper.oldestSafeRollbackVersion,
    "2026.1116.1",
  );

  // No committed ledger, but the working tree claims an EARLIER release shipped: that floor exists
  // only in this checkout, so it cannot be sealed into a signed bundle.
  const root2 = fixture(t);
  const inherited = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };
  const source2 = writeSource(root2, inherited);
  refusal("KEEPER_LEDGER_UNCOMMITTED", () =>
    keeperRollbackManifest({
      appVersion: "2026.1117.1",
      ledgerPath: join(source2, "packaging", "keeper", "releases.json"),
      committedLedgerText: null,
    }),
  );
});

test("a working tree that rewrites or misappends committed history is refused", (t) => {
  const root = fixture(t);
  const prior = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };

  // A committed entry removed from the working tree.
  const removed = writeSource(mkdirRecursive(root, "removed"), EMPTY_LEDGER);
  refusal("KEEPER_LEDGER_DIRTY", () =>
    keeperRollbackManifest({
      appVersion: "2026.1117.1",
      ledgerPath: join(removed, "packaging", "keeper", "releases.json"),
      committedLedgerText: committed(prior),
    }),
  );

  // A committed floor silently rewritten to an older build.
  const rewritten = writeSource(mkdirRecursive(root, "rewritten"), {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1115.1" },
    ],
  });
  refusal("KEEPER_LEDGER_DIRTY", () =>
    keeperRollbackManifest({
      appVersion: "2026.1117.1",
      ledgerPath: join(rewritten, "packaging", "keeper", "releases.json"),
      committedLedgerText: committed(prior),
    }),
  );

  // An uncommitted append for a different release than the one being built.
  const foreign = writeSource(mkdirRecursive(root, "foreign"), {
    schemaVersion: 1,
    keeperReleases: [
      ...prior.keeperReleases,
      { appVersion: "2026.1117.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  });
  refusal("KEEPER_LEDGER_DIRTY", () =>
    keeperRollbackManifest({
      appVersion: "2026.1118.1",
      ledgerPath: join(foreign, "packaging", "keeper", "releases.json"),
      committedLedgerText: committed(prior),
    }),
  );
});

test("an existing entry for this release must state the floor the committed entries derive", (t) => {
  const root = fixture(t);
  const prior = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };
  // The append exists but records a floor above the committed derivation: publishing it would put a
  // different floor on the release page than the signed bundle carries.
  const disagreeing = writeSource(root, {
    schemaVersion: 1,
    keeperReleases: [
      ...prior.keeperReleases,
      { appVersion: "2026.1117.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1117.1" },
    ],
  });
  refusal("KEEPER_LEDGER_DIRTY", () =>
    keeperRollbackManifest({
      appVersion: "2026.1117.1",
      ledgerPath: join(disagreeing, "packaging", "keeper", "releases.json"),
      committedLedgerText: committed(prior),
    }),
  );
});

// ---------------------------------------------------------------------------------------------
// Shadowing: `shipped_manifest_path` prefers Resources/latest.json over Resources/keeper/latest.json
// ---------------------------------------------------------------------------------------------

test("a bundled updater manifest that agrees is tolerated", (t) => {
  const root = fixture(t);
  const ledger = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };
  const source = writeSource(root, ledger);
  const appPath = writeApp(root, "2026.1117.1");
  const seam = {
    appPath,
    appVersion: "2026.1117.1",
    sourceDir: source,
    committedLedgerText: committed(ledger),
  };
  writeKeeperRollbackManifest(seam);
  const updaterPath = bundledUpdaterManifestPath(appPath);
  writeFileSync(
    updaterPath,
    `${JSON.stringify({ version: "2026.1117.1", keeper: { oldestSafeRollbackVersion: "2026.1116.1" } }, null, 2)}\n`,
  );
  const checked = assertFloorNotShadowed({ appPath });
  assert.equal(checked.floor, "2026.1116.1");
  assert.equal(checked.updaterPath, updaterPath);
});

test("a bundled updater manifest with a different or missing floor is refused", (t) => {
  const root = fixture(t);
  const ledger = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };
  const source = writeSource(root, ledger);
  const appPath = writeApp(root, "2026.1117.1");
  const seam = {
    appPath,
    appVersion: "2026.1117.1",
    sourceDir: source,
    committedLedgerText: committed(ledger),
  };
  writeKeeperRollbackManifest(seam);
  const updaterPath = bundledUpdaterManifestPath(appPath);

  // An older floor in the file the daemon reads first would silently permit an unsafe rollback.
  writeFileSync(
    updaterPath,
    `${JSON.stringify({ version: "2026.1117.1", keeper: { oldestSafeRollbackVersion: "2026.1115.1" } }, null, 2)}\n`,
  );
  refusal("KEEPER_FLOOR_SHADOWED", () => assertFloorNotShadowed({ appPath }));
  // And a re-run of the write path must not leave that state behind, either.
  refusal("KEEPER_FLOOR_SHADOWED", () => writeKeeperRollbackManifest(seam));

  // No keeper block at all: the daemon would answer NoFloorRecorded and drop the check.
  writeFileSync(updaterPath, `${JSON.stringify({ version: "2026.1117.1", platforms: {} }, null, 2)}\n`);
  refusal("KEEPER_FLOOR_SHADOWED", () => assertFloorNotShadowed({ appPath }));
});

// ---------------------------------------------------------------------------------------------
// Publication must read the ACTUAL published floor, not the ledger's expectation.
// ---------------------------------------------------------------------------------------------

test("publication compares against the floor the assembled latest.json really states", (t) => {
  const root = fixture(t);
  const ledger = {
    schemaVersion: 1,
    keeperReleases: [
      { appVersion: "2026.1116.1", protocolMajor: 1, oldestSafeRollbackVersion: "2026.1116.1" },
    ],
  };
  const source = writeSource(root, ledger);
  const appPath = writeApp(root, "2026.1117.1");
  writeKeeperRollbackManifest({
    appPath,
    appVersion: "2026.1117.1",
    sourceDir: source,
    committedLedgerText: committed(ledger),
  });

  const latestJsonPath = join(root, "latest.json");
  writeFileSync(
    latestJsonPath,
    `${JSON.stringify({ version: "2026.1117.1", keeper: { oldestSafeRollbackVersion: "2026.1116.1" } }, null, 2)}\n`,
  );
  assert.equal(publishedFloor(latestJsonPath).floor, "2026.1116.1");
  assert.equal(
    assertKeeperFloorMatchesPublished({ appPath, latestJsonPath }).oldestSafeRollbackVersion,
    "2026.1116.1",
  );

  // The published document is the authority: when it disagrees, publication stops.
  writeFileSync(
    latestJsonPath,
    `${JSON.stringify({ version: "2026.1117.1", keeper: { oldestSafeRollbackVersion: "2026.1115.1" } }, null, 2)}\n`,
  );
  refusal("KEEPER_FLOOR_MISMATCH", () => assertKeeperFloorMatchesPublished({ appPath, latestJsonPath }));

  // A published manifest with no keeper block is a refusal, not "no floor".
  writeFileSync(latestJsonPath, `${JSON.stringify({ version: "2026.1117.1", platforms: {} }, null, 2)}\n`);
  refusal("KEEPER_PUBLISHED_FLOOR_MISSING", () => publishedFloor(latestJsonPath));
});
