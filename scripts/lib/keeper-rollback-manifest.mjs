// The rollback floor the shipped macOS bundle carries, at `Contents/Resources/keeper/latest.json`.
//
// `daemon::rollback` (`src-tauri/src/daemon/rollback.rs::shipped_manifest_path`) looks for the
// release manifest beside the executable and reads its `keeper.oldestSafeRollbackVersion`. When no
// such file is in the bundle it answers `KEEPER_ROLLBACK_FLOOR_UNVERIFIED` and refuses every
// rollback, because an unknown minimum is not a permissive minimum. Nothing in the bundler emitted
// that file: `latest.json` is assembled for the updater and published to the release page, never
// copied into the app. So every macOS packaging path plants it, or the rollback feature can never
// work on a shipped build while still looking implemented.
//
// The floor here is NOT derived again. `scripts/lib/release-keeper.mjs::keeperManifest` is the one
// derivation, sourced from the checked-in ledger, and it is the same call `build-latest-json.mjs`
// makes for the published `latest.json`. Re-deriving would let the bundle and the release page
// disagree about the minimum safe build, which is exactly the pair a rollback compares.
//
// The floor is derived from the ledger entries that were ALREADY COMMITTED before this release, never
// from the entry this release will add. The distinction matters because the publish path appends the
// entry at publication time, after the bundle has been signed: requiring the release's own entry to
// be committed first would be circular, since the commit cannot be made until the release is decided
// and the release cannot be built until the commit exists. So:
//
//   * Pre-existing entries must be committed. Their floors are what a rollback compares against, and
//     a floor that only exists in one checkout is not a floor any user's build can agree with.
//   * This release's entry need not be committed, and need not exist at all. The floor it would
//     record is derived from the committed entries, so the two agree by construction.
//
// Concretely, the build reads the committed ledger for the floor, then uses the working-tree ledger
// only to prove it does not contradict that commit:
//
//   * `KEEPER_LEDGER_DIRTY` - the working tree does not begin with the committed entries, or appends
//     something other than this release, or records this release under a different floor than the
//     committed entries derive. That last case is the one that would let the published `latest.json`
//     and the signed bundle state different minimums.
//   * `KEEPER_LEDGER_UNCOMMITTED` - there is no committed ledger yet (the first keeper release, whose
//     ledger file is added by that same release) *and* the working tree already claims another
//     release shipped. Pre-existing history that no commit records is not a trustworthy floor.
//   * `KEEPER_LEDGER_PENDING_INTENT` / `KEEPER_LEDGER_LOCKED` - the publish intent file or the ledger
//     lock is still on disk, so a publish is in flight or died mid-flight and the ledger is not a
//     settled statement of the floor.
//
// Plus the ledger's own contract, enforced by `readKeeperLedger`: missing file, unknown keys, schema
// drift, non-increasing releases, a floor that moves backwards, a floor above its own release, and a
// protocol major this packager does not speak are all refusals.
//
// Two documents can state a floor in one bundle, and `shipped_manifest_path` prefers
// `Resources/latest.json` over `Resources/keeper/latest.json`. A bundled updater manifest therefore
// shadows this one: if it carries a different floor the daemon silently uses the other value, and if
// it carries no `keeper` block the daemon answers `NoFloorRecorded` and drops the floor check
// entirely. `assertFloorNotShadowed` refuses both, so the two files cannot disagree.
//
// Nothing here reads the installed app. An installed version without a keeper proves nothing about
// whether any keeper release ever shipped, so it can never stand in for the ledger.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { isAbsolute, join, relative, sep } from "node:path";

import { KEEPER_MANIFEST_NAME, KEEPER_RESOURCE_DIR } from "./keeper-bundle.mjs";
import {
  isKeeperVersion,
  KEEPER_MANIFEST_KEY,
  KEEPER_POLICY_SCHEMA_VERSION,
  keeperManifest,
  OLDEST_SAFE_ROLLBACK_KEY,
  parseKeeperLedger,
} from "./release-keeper.mjs";

/** Where the checked-in ledger lives, relative to the source tree that is being bundled. */
export const KEEPER_LEDGER_REL = join("packaging", "keeper", "releases.json");
/** The filename `daemon::rollback` searches for inside `Contents/Resources/keeper`. */
export const KEEPER_ROLLBACK_MANIFEST_NAME = "latest.json";

/** Typed refusal: the code leads the message so a build failure names the missing fact. */
export class KeeperRollbackManifestError extends Error {
  constructor(code, message, detail = {}) {
    super(`${code}: ${message}`);
    this.name = "KeeperRollbackManifestError";
    this.code = code;
    this.detail = detail;
  }
}

/** The bundle directory the daemon reads, from the app path. */
export function keeperRollbackManifestPath(appPath) {
  return join(appPath, "Contents", "Resources", KEEPER_RESOURCE_DIR, KEEPER_ROLLBACK_MANIFEST_NAME);
}

/**
 * The updater manifest a bundler may have laid beside the keeper directory.
 *
 * Nothing in this repository copies `latest.json` into the bundle (`tauri.conf.json`'s
 * `bundle.resources` lists only `ui/dist`, `helpers` and `mhc`), but `daemon::rollback` would read
 * this path first if anything ever did, so its presence is checked rather than assumed away.
 */
export function bundledUpdaterManifestPath(appPath) {
  return join(appPath, "Contents", "Resources", KEEPER_ROLLBACK_MANIFEST_NAME);
}

/** The ledger a source tree ships: `<sourceDir>/packaging/keeper/releases.json`. */
export function shippedLedgerPath(sourceDir) {
  return join(sourceDir, KEEPER_LEDGER_REL);
}

/**
 * The two on-disk artifacts the publish path leaves around a ledger append.
 *
 * Both are read here and never written: `scripts/release-local.mjs` owns creating and clearing them.
 * Their presence at packaging time means a publish is in flight or was interrupted, which is a
 * refusal exactly because the ledger on disk is then not yet a settled floor.
 */
export function keeperLedgerArtifacts(ledgerPath) {
  return { pending: `${ledgerPath}.pending`, lock: `${ledgerPath}.lock` };
}

/** One ledger entry in a stable text form, for prefix comparison. */
function entryKey(release) {
  return JSON.stringify([release.appVersion, release.protocolMajor, release.oldestSafeRollbackVersion]);
}

/** True when `whole` begins with every entry of `prefix`, in order and with equal values. */
function startsWithEntries(whole, prefix) {
  if (prefix.length > whole.length) return false;
  return prefix.every((release, index) => entryKey(release) === entryKey(whole[index]));
}

/**
 * The ledger text committed at HEAD in `sourceDir`.
 *
 * `null` means git reports the path absent from HEAD, which is "never committed yet" - not "empty".
 * Every other git failure is typed, because a floor derived from bytes whose provenance is unknown is
 * the silent-advance failure this whole check exists to prevent.
 */
export function readCommittedLedgerText({ sourceDir, ledgerPath, spawn = spawnSync }) {
  const relPath = relative(sourceDir, ledgerPath);
  if (relPath === "" || relPath.startsWith("..") || isAbsolute(relPath)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_LEDGER_OUTSIDE_SOURCE",
      `${ledgerPath} is outside the bundled source tree ${sourceDir}, so its committed bytes cannot be read`,
      { ledgerPath, sourceDir },
    );
  }
  const result = spawn("git", ["show", `HEAD:${relPath.split(sep).join("/")}`], {
    cwd: sourceDir,
    encoding: "utf8",
  });
  if (result.error) {
    throw new KeeperRollbackManifestError(
      "KEEPER_LEDGER_COMMIT_UNKNOWN",
      `cannot run git in ${sourceDir} to read ${relPath} at HEAD: ${result.error.message}`,
      { ledgerPath, sourceDir },
    );
  }
  if (result.status === 0) return result.stdout;
  const stderr = result.stderr ?? "";
  if (/does not exist in|exists on disk, but not in|unknown revision|path '.*' does not exist/.test(stderr)) {
    return null;
  }
  if (/not a git repository/i.test(stderr)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_LEDGER_COMMIT_UNKNOWN",
      `${sourceDir} is not a git repository, so the ledger's committed bytes cannot be read and the floor cannot be trusted`,
      { ledgerPath, sourceDir },
    );
  }
  throw new KeeperRollbackManifestError(
    "KEEPER_LEDGER_COMMIT_UNKNOWN",
    `cannot read ${relPath} at HEAD in ${sourceDir}: ${stderr.trim()}`,
    { ledgerPath, sourceDir },
  );
}

/**
 * The bundle document for `appVersion`, derived from the ledger entries committed before it.
 *
 * `committedLedgerText` is a seam for tests and for a caller that already read HEAD; production call
 * sites omit it and get `readCommittedLedgerText({ sourceDir })`.
 *
 * The working-tree ledger may carry this release's entry, or may not: the publish path appends it
 * after the bundle is signed. What it may not do is contradict the commit or state a different floor
 * for this release than the committed entries derive.
 */
export function keeperRollbackManifest({
  appVersion,
  ledgerPath,
  sourceDir = null,
  committedLedgerText,
  spawn = spawnSync,
}) {
  if (!isKeeperVersion(appVersion)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_ROLLBACK_VERSION_UNREADABLE",
      `cannot write a rollback floor for ${JSON.stringify(appVersion ?? null)}; it is not a three-component version`,
    );
  }
  if (typeof ledgerPath !== "string" || ledgerPath === "") {
    throw new KeeperRollbackManifestError(
      "KEEPER_LEDGER_MISSING",
      "no keeper ledger path was given, so the rollback floor cannot be derived",
    );
  }

  const artifacts = keeperLedgerArtifacts(ledgerPath);
  if (existsSync(artifacts.pending)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_LEDGER_PENDING_INTENT",
      `${artifacts.pending} still exists, so a keeper publish is in flight or died between recording the ledger and confirming its release went public. ` +
        "The ledger on disk may describe a draft, so a signed bundle must not seal a floor from it",
      { ledgerPath, pending: artifacts.pending },
    );
  }
  if (existsSync(artifacts.lock)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_LEDGER_LOCKED",
      `${artifacts.lock} is held, so another publish is writing the ledger right now; the floor is not stable enough to seal into a bundle`,
      { ledgerPath, lock: artifacts.lock },
    );
  }
  if (!existsSync(ledgerPath)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_LEDGER_MISSING",
      `no keeper ledger at ${ledgerPath}; a bundle that cannot state the oldest safe rollback version must not ship`,
      { ledgerPath },
    );
  }

  // Parse both sides through the ledger's own validator, so a copy that violates the contract is a
  // refusal here rather than a floor the release later denies.
  let onDisk;
  try {
    onDisk = parseKeeperLedger(readFileSync(ledgerPath, "utf8"));
  } catch (error) {
    throw new KeeperRollbackManifestError(
      "KEEPER_LEDGER_UNTRUSTED",
      `the keeper ledger at ${ledgerPath} cannot be trusted to state the rollback floor: ${error.message}`,
      { ledgerPath, cause: error.message },
    );
  }

  let committedText = committedLedgerText;
  if (committedText === undefined) {
    if (!sourceDir) {
      throw new KeeperRollbackManifestError(
        "KEEPER_LEDGER_COMMIT_UNKNOWN",
        `no source tree was given for ${ledgerPath}, so the ledger's committed bytes cannot be read and the floor cannot be trusted`,
        { ledgerPath },
      );
    }
    committedText = readCommittedLedgerText({ sourceDir, ledgerPath, spawn });
  }

  let committed;
  if (committedText === null) {
    // No committed ledger at all. That is only legitimate for the release that introduces the ledger,
    // so the working tree must not already claim some other keeper release shipped - that history
    // would be a floor no commit records.
    const stranger = onDisk.keeperReleases.find((release) => release.appVersion !== appVersion);
    if (stranger) {
      throw new KeeperRollbackManifestError(
        "KEEPER_LEDGER_UNCOMMITTED",
        `${ledgerPath} is not committed at HEAD but already records keeper release ${stranger.appVersion}. ` +
          "A floor that exists only in this checkout cannot be sealed into a signed bundle: commit the pre-existing entries first.",
        { ledgerPath, uncommitted: stranger.appVersion },
      );
    }
    committed = { schemaVersion: KEEPER_POLICY_SCHEMA_VERSION, keeperReleases: [] };
  } else {
    try {
      committed = parseKeeperLedger(committedText);
    } catch (error) {
      throw new KeeperRollbackManifestError(
        "KEEPER_LEDGER_UNTRUSTED",
        `the ledger committed at HEAD for ${ledgerPath} is not a trusted statement of the rollback floor: ${error.message}`,
        { ledgerPath, cause: error.message },
      );
    }
    // Every committed entry must still be present, in order, with equal values. An append is fine;
    // an edit, a removal or a reordering means the floor history moved under a signed bundle.
    if (!startsWithEntries(onDisk.keeperReleases, committed.keeperReleases)) {
      throw new KeeperRollbackManifestError(
        "KEEPER_LEDGER_DIRTY",
        `${ledgerPath} does not begin with its committed entries from HEAD, so this checkout rewrites keeper release history. ` +
          "Commit the settled ledger before sealing a floor into a bundle.",
        {
          ledgerPath,
          committed: committed.keeperReleases.map(entryKey),
          workingTree: onDisk.keeperReleases.map(entryKey),
        },
      );
    }
    // The only permitted append is this release.
    const appended = onDisk.keeperReleases.slice(committed.keeperReleases.length);
    const foreign = appended.find((release) => release.appVersion !== appVersion);
    if (foreign) {
      throw new KeeperRollbackManifestError(
        "KEEPER_LEDGER_DIRTY",
        `${ledgerPath} appends keeper release ${foreign.appVersion}, which is not this release (${appVersion}) and is not committed. ` +
          "An uncommitted floor for another release may describe a build that never shipped.",
        { ledgerPath, appended: foreign.appVersion, appVersion },
      );
    }
  }

  let keeper;
  try {
    keeper = keeperManifest({ appVersion, ledger: committed });
  } catch (error) {
    throw new KeeperRollbackManifestError(
      "KEEPER_LEDGER_UNTRUSTED",
      `the committed keeper ledger cannot state a rollback floor for ${appVersion}: ${error.message}`,
      { ledgerPath, cause: error.message },
    );
  }

  // If the working tree already records this release - a re-run after a publish, or a build cut from
  // a checkout that has the append - it must record exactly the floor the committed entries derive.
  // Otherwise the entry would rewrite the floor the bundle just sealed.
  const ownEntry = onDisk.keeperReleases.find((release) => release.appVersion === appVersion);
  if (ownEntry) {
    const derived = keeper[OLDEST_SAFE_ROLLBACK_KEY];
    if (ownEntry[OLDEST_SAFE_ROLLBACK_KEY] !== derived) {
      throw new KeeperRollbackManifestError(
        "KEEPER_LEDGER_DIRTY",
        `${ledgerPath} records ${appVersion} with oldestSafeRollbackVersion ${JSON.stringify(ownEntry[OLDEST_SAFE_ROLLBACK_KEY])} ` +
          `but the committed entries derive ${JSON.stringify(derived)}; the published manifest and the signed bundle would state different floors`,
        { ledgerPath, recorded: ownEntry[OLDEST_SAFE_ROLLBACK_KEY], derived },
      );
    }
  }
  return { version: appVersion, [KEEPER_MANIFEST_KEY]: keeper };
}

/**
 * Writes the floor manifest into an app bundle that already carries a keeper payload.
 *
 * Ordering is the whole point: the pre-existing ledger entries must be committed, and the keeper
 * binary and its `keeper.json` must be installed, before the floor document exists. This release's
 * own entry is deliberately not required - the publish path appends it after the bundle is signed -
 * so building before the append is the normal case rather than an error.
 */
export function writeKeeperRollbackManifest({
  appPath,
  appVersion,
  sourceDir = null,
  ledgerPath = null,
  committedLedgerText,
  spawn = spawnSync,
}) {
  const resolvedLedger = ledgerPath ?? (sourceDir ? shippedLedgerPath(sourceDir) : null);
  // Derive first: a ledger problem must fail before anything is written into the bundle.
  const document = keeperRollbackManifest({
    appVersion,
    ledgerPath: resolvedLedger,
    sourceDir,
    committedLedgerText,
    spawn,
  });

  const resourceDir = join(appPath, "Contents", "Resources", KEEPER_RESOURCE_DIR);
  const bundledManifestPath = join(resourceDir, KEEPER_MANIFEST_NAME);
  if (!existsSync(bundledManifestPath)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_PAYLOAD_ABSENT",
      `no ${KEEPER_MANIFEST_NAME} under ${resourceDir}, so no keeper payload is bundled and a rollback floor would describe a keeper that is not there`,
      { resourceDir },
    );
  }
  let bundled;
  try {
    bundled = JSON.parse(readFileSync(bundledManifestPath, "utf8"));
  } catch (error) {
    throw new KeeperRollbackManifestError(
      "KEEPER_PAYLOAD_UNUSABLE",
      `${bundledManifestPath} is not readable JSON: ${error.message}`,
      { bundledManifestPath },
    );
  }
  if (bundled.version !== appVersion) {
    throw new KeeperRollbackManifestError(
      "KEEPER_PAYLOAD_VERSION_MISMATCH",
      `${bundledManifestPath} records generation ${JSON.stringify(bundled.version)} but this release is ${appVersion}; ` +
        "the floor would describe a generation the bundle does not carry",
      { recorded: bundled.version, appVersion },
    );
  }

  // Refuse before writing: a shadowing updater manifest is a property of the bundle this document is
  // about to join, so discovering it after the write would leave a floor file behind in a bundle that
  // cannot ship.
  refuseIfShadowed({ appPath, expectedFloor: document[KEEPER_MANIFEST_KEY][OLDEST_SAFE_ROLLBACK_KEY] });

  const manifestPath = keeperRollbackManifestPath(appPath);
  mkdirSync(resourceDir, { recursive: true });
  writeFileSync(manifestPath, `${JSON.stringify(document, null, 2)}\n`);
  return { manifestPath, document, ledgerPath: resolvedLedger };
}

/**
 * Asserts a bundle's floor manifest is exactly what the committed ledger derives.
 *
 * The comparison is whole-document: a floor that is merely *valid* could still differ from the one
 * the ledger states, and a bundle whose floor drifted from the published `latest.json` would let a
 * user roll back past the version the release promised was safe.
 */
export function assertKeeperRollbackManifest({
  appPath,
  appVersion,
  sourceDir = null,
  ledgerPath = null,
  committedLedgerText,
  spawn = spawnSync,
}) {
  const manifestPath = keeperRollbackManifestPath(appPath);
  if (!existsSync(manifestPath)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_ROLLBACK_MANIFEST_MISSING",
      `no ${KEEPER_ROLLBACK_MANIFEST_NAME} at ${manifestPath}; daemon::rollback would answer KEEPER_ROLLBACK_FLOOR_UNVERIFIED for every attempt`,
      { manifestPath },
    );
  }
  const resolvedLedger = ledgerPath ?? (sourceDir ? shippedLedgerPath(sourceDir) : null);
  const expected = keeperRollbackManifest({
    appVersion,
    ledgerPath: resolvedLedger,
    sourceDir,
    committedLedgerText,
    spawn,
  });
  let actual;
  try {
    actual = JSON.parse(readFileSync(manifestPath, "utf8"));
  } catch (error) {
    throw new KeeperRollbackManifestError(
      "KEEPER_ROLLBACK_MANIFEST_UNREADABLE",
      `${manifestPath} is not JSON: ${error.message}`,
      { manifestPath },
    );
  }
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_ROLLBACK_MANIFEST_MISMATCH",
      `${manifestPath} does not match the ledger-derived floor: bundled ${JSON.stringify(actual)}, ledger states ${JSON.stringify(expected)}`,
      { manifestPath, actual, expected, ledgerPath: resolvedLedger },
    );
  }
  return { manifestPath, manifest: actual, ledgerPath: resolvedLedger };
}

/**
 * Reads a published manifest and returns the floor it actually states.
 *
 * `shipped_manifest_path` consults `Resources/latest.json` before `Resources/keeper/latest.json`, and
 * the published release page carries the same `keeper` block. "Publication reads the actual published
 * floor" therefore means: whatever `latest.json` states, not what the ledger was expected to say.
 * A manifest with no `keeper` block is refused rather than treated as "no floor", because on the
 * release page that would silently withdraw a floor the bundle asserts.
 */
export function publishedFloor(latestJsonPath) {
  if (!existsSync(latestJsonPath)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_PUBLISHED_MANIFEST_MISSING",
      `no assembled manifest at ${latestJsonPath}, so there is no published floor to read`,
      { latestJsonPath },
    );
  }
  let published;
  try {
    published = JSON.parse(readFileSync(latestJsonPath, "utf8"));
  } catch (error) {
    throw new KeeperRollbackManifestError(
      "KEEPER_ROLLBACK_MANIFEST_UNREADABLE",
      `${latestJsonPath} is not JSON: ${error.message}`,
      { latestJsonPath },
    );
  }
  const floor = published?.[KEEPER_MANIFEST_KEY]?.[OLDEST_SAFE_ROLLBACK_KEY];
  if (typeof floor !== "string") {
    throw new KeeperRollbackManifestError(
      "KEEPER_PUBLISHED_FLOOR_MISSING",
      `${latestJsonPath} carries no keeper.${OLDEST_SAFE_ROLLBACK_KEY}, so the release states no floor while the bundle does`,
      { latestJsonPath },
    );
  }
  if (!isKeeperVersion(floor)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_PUBLISHED_FLOOR_MISSING",
      `${latestJsonPath} states keeper.${OLDEST_SAFE_ROLLBACK_KEY} ${JSON.stringify(floor)}, which is not a three-component version`,
      { latestJsonPath, floor },
    );
  }
  return { latestJsonPath, floor, manifest: published };
}

/**
 * Whether a `Resources/latest.json` in this bundle would shadow the keeper floor, given the floor
 * this bundle is about to state.
 *
 * Kept private and floor-explicit so the write path can refuse *before* writing, and so the exported
 * assertion can reuse exactly one definition of "shadowed".
 */
function refuseIfShadowed({ appPath, expectedFloor }) {
  const updaterPath = bundledUpdaterManifestPath(appPath);
  if (!existsSync(updaterPath)) return null;
  let updater;
  try {
    updater = JSON.parse(readFileSync(updaterPath, "utf8"));
  } catch (error) {
    throw new KeeperRollbackManifestError(
      "KEEPER_ROLLBACK_MANIFEST_UNREADABLE",
      `${updaterPath} is not JSON: ${error.message}`,
      { updaterPath },
    );
  }
  const keeperBlock = updater?.[KEEPER_MANIFEST_KEY];
  if (keeperBlock === undefined) {
    throw new KeeperRollbackManifestError(
      "KEEPER_FLOOR_SHADOWED",
      `${updaterPath} is bundled ahead of ${keeperRollbackManifestPath(appPath)} and carries no keeper block, ` +
        "so daemon::rollback would answer NoFloorRecorded and drop the floor check for every rollback",
      { updaterPath },
    );
  }
  const shadowFloor = keeperBlock?.[OLDEST_SAFE_ROLLBACK_KEY];
  if (shadowFloor !== expectedFloor) {
    throw new KeeperRollbackManifestError(
      "KEEPER_FLOOR_SHADOWED",
      `${updaterPath} is bundled ahead of ${keeperRollbackManifestPath(appPath)} and states ${JSON.stringify(shadowFloor)} ` +
        `while the keeper manifest states ${JSON.stringify(expectedFloor)}; daemon::rollback would use the updater's value, ` +
        "so the two must agree or the shadowing file must not ship",
      { updaterPath, shadowFloor, expectedFloor },
    );
  }
  return { updaterPath, floor: expectedFloor };
}

/**
 * Refuses a bundle in which `Resources/latest.json` would shadow `Resources/keeper/latest.json`.
 *
 * The daemon takes the first of those two files that exists, so a bundled updater manifest decides
 * the floor whenever it is present: a different floor silently wins, and a missing `keeper` block
 * turns into `NoFloorRecorded` and drops the floor check altogether. Both are refusals here.
 */
export function assertFloorNotShadowed({ appPath }) {
  const manifestPath = keeperRollbackManifestPath(appPath);
  if (!existsSync(manifestPath)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_ROLLBACK_MANIFEST_MISSING",
      `no ${KEEPER_ROLLBACK_MANIFEST_NAME} at ${manifestPath}, so the bundle states no floor to protect from shadowing`,
      { manifestPath },
    );
  }
  let own;
  try {
    own = JSON.parse(readFileSync(manifestPath, "utf8"));
  } catch (error) {
    throw new KeeperRollbackManifestError(
      "KEEPER_ROLLBACK_MANIFEST_UNREADABLE",
      `${manifestPath} is not JSON: ${error.message}`,
      { manifestPath },
    );
  }
  const ownFloor = own?.[KEEPER_MANIFEST_KEY]?.[OLDEST_SAFE_ROLLBACK_KEY];
  const checked = refuseIfShadowed({ appPath, expectedFloor: ownFloor });
  return { manifestPath, updaterPath: checked?.updaterPath ?? null, floor: ownFloor };
}

/**
 * Compares a built bundle's floor with the floor a published `latest.json` actually states.
 *
 * This is the publish-side half, for the coordinator to call before anything goes public: the bundle
 * a user installs and the manifest a user's updater reads must state one floor. It reads the two
 * documents and writes nothing, so it is safe anywhere in the publish sequence.
 */
export function assertKeeperFloorMatchesPublished({ appPath, latestJsonPath }) {
  const manifestPath = keeperRollbackManifestPath(appPath);
  if (!existsSync(manifestPath)) {
    throw new KeeperRollbackManifestError(
      "KEEPER_ROLLBACK_MANIFEST_MISSING",
      `no ${KEEPER_ROLLBACK_MANIFEST_NAME} at ${manifestPath}, so the bundle states no floor to compare`,
      { manifestPath },
    );
  }
  let bundled;
  try {
    bundled = JSON.parse(readFileSync(manifestPath, "utf8"));
  } catch (error) {
    throw new KeeperRollbackManifestError(
      "KEEPER_ROLLBACK_MANIFEST_UNREADABLE",
      `${manifestPath} is not JSON: ${error.message}`,
      { manifestPath },
    );
  }
  const bundledFloor = bundled?.[KEEPER_MANIFEST_KEY]?.[OLDEST_SAFE_ROLLBACK_KEY];
  const { floor: publishedStateFloor } = publishedFloor(latestJsonPath);
  if (bundledFloor !== publishedStateFloor) {
    throw new KeeperRollbackManifestError(
      "KEEPER_FLOOR_MISMATCH",
      `the bundle states ${JSON.stringify(bundledFloor)} but ${latestJsonPath} publishes ${JSON.stringify(publishedStateFloor)}; ` +
        "a rollback would compare the bundle's floor against a release page that promises a different minimum",
      { manifestPath, latestJsonPath, bundledFloor, publishedFloor: publishedStateFloor },
    );
  }
  return { manifestPath, latestJsonPath, oldestSafeRollbackVersion: publishedStateFloor };
}
