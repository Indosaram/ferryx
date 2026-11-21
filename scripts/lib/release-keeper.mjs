// The `oldestSafeRollbackVersion` a release publishes in `latest.json`.
//
// The plan requires the oldest safe rollback version to be recorded in the release manifest. The
// value is derived from the checked-in ledger (`packaging/keeper/releases.json`) rather than guessed
// from the release being cut, because the floor is a property of the *first* keeper-capable build:
// every later release keeps pointing at it, and a release older than it can never service
// keeper-owned sessions. Deriving it this way also keeps a published manifest reproducible after
// the ledger records the release, which `verifyReleaseRun`'s byte comparison depends on.
//
// The refusal itself lives where the sessions are: `src-tauri/src/daemon/keeper_launchd.rs`
// (`rollback_gate`) fails closed while any keeper-owned session exists and names the count. This
// module only publishes the floor that refusal compares against.
import { readFileSync } from "node:fs";

export const KEEPER_RETAINED_GENERATIONS = 3;
export const KEEPER_PROTOCOL_MAJOR = 1;
export const KEEPER_POLICY_SCHEMA_VERSION = 1;
export const KEEPER_MANIFEST_KEY = "keeper";
export const OLDEST_SAFE_ROLLBACK_KEY = "oldestSafeRollbackVersion";

/** Numeric comparison of `<major>.<minor>.<patch>`, with a lexical tiebreak on `-pre`/`+build`. */
export function compareKeeperVersions(left, right) {
  const split = (text) => {
    const [core, ...rest] = String(text).split(/[-+]/);
    const numbers = core.split(".").map((part) => {
      const value = Number.parseInt(part, 10);
      return Number.isNaN(value) ? 0 : value;
    });
    while (numbers.length < 3) numbers.push(0);
    return { numbers, suffix: rest.map((part) => `-${part}`).join("") };
  };
  const a = split(left);
  const b = split(right);
  for (let index = 0; index < 3; index += 1) {
    if (a.numbers[index] !== b.numbers[index]) return a.numbers[index] < b.numbers[index] ? -1 : 1;
  }
  if (a.suffix === b.suffix) return 0;
  return a.suffix < b.suffix ? -1 : 1;
}

/** A directory name usable as a generation: three numeric components, optional suffix. */
export function isKeeperVersion(name) {
  if (typeof name !== "string" || name === "") return false;
  const core = name.split(/[-+]/)[0];
  const parts = core.split(".");
  return parts.length === 3 && parts.every((part) => part !== "" && /^[0-9]+$/.test(part));
}

function readJsonFile(path) {
  let text;
  try {
    text = readFileSync(path, "utf8");
  } catch (error) {
    throw new Error(`cannot read ${path}: ${error.message}`);
  }
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new Error(`${path} is not valid JSON: ${error.message}`);
  }
}

/**
 * Validates the checked-in keeper ledger: every release that shipped a keeper, oldest first.
 *
 * The ledger is the only source of the rollback floor. It starts empty because no release has
 * shipped a keeper yet, so the first keeper release publishes itself as the floor instead of a
 * fabricated version that never existed.
 */
export function parseKeeperLedger(value) {
  const ledger = typeof value === "string" ? JSON.parse(value) : value;
  if (!ledger || typeof ledger !== "object" || Array.isArray(ledger)) {
    throw new Error("keeper ledger must be a JSON object");
  }
  const allowed = new Set(["schemaVersion", "keeperReleases"]);
  for (const key of Object.keys(ledger)) {
    if (!allowed.has(key)) throw new Error(`keeper ledger contains unexpected property: ${key}`);
  }
  if (ledger.schemaVersion !== KEEPER_POLICY_SCHEMA_VERSION) {
    throw new Error(`keeper ledger schemaVersion must be ${KEEPER_POLICY_SCHEMA_VERSION}, got ${JSON.stringify(ledger.schemaVersion)}`);
  }
  if (!Array.isArray(ledger.keeperReleases)) {
    throw new Error("keeper ledger keeperReleases must be an array");
  }
  const releases = [];
  let previous = null;
  for (const entry of ledger.keeperReleases) {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
      throw new Error("keeper ledger release entries must be objects");
    }
    const entryKeys = new Set(["appVersion", "protocolMajor", "oldestSafeRollbackVersion"]);
    for (const key of Object.keys(entry)) {
      if (!entryKeys.has(key)) throw new Error(`keeper ledger release entry contains unexpected property: ${key}`);
    }
    if (!isKeeperVersion(entry.appVersion)) {
      throw new Error(`keeper ledger release appVersion ${JSON.stringify(entry.appVersion)} is not a three-component version`);
    }
    if (entry.protocolMajor !== KEEPER_PROTOCOL_MAJOR) {
      throw new Error(
        `keeper ledger release ${entry.appVersion} speaks protocol major ${JSON.stringify(entry.protocolMajor)}; this packager speaks ${KEEPER_PROTOCOL_MAJOR}`,
      );
    }
    if (!isKeeperVersion(entry.oldestSafeRollbackVersion)) {
      throw new Error(
        `keeper ledger release ${entry.appVersion} has no usable oldestSafeRollbackVersion (${JSON.stringify(entry.oldestSafeRollbackVersion)})`,
      );
    }
    if (compareKeeperVersions(entry.oldestSafeRollbackVersion, entry.appVersion) > 0) {
      throw new Error(`keeper ledger release ${entry.appVersion} cannot roll back to a newer version ${entry.oldestSafeRollbackVersion}`);
    }
    if (previous && compareKeeperVersions(entry.appVersion, previous.appVersion) <= 0) {
      throw new Error(`keeper ledger releases must be strictly increasing: ${previous.appVersion} then ${entry.appVersion}`);
    }
    if (previous && compareKeeperVersions(entry.oldestSafeRollbackVersion, previous.oldestSafeRollbackVersion) < 0) {
      throw new Error(
        `keeper ledger floor may not move backwards: ${previous.oldestSafeRollbackVersion} then ${entry.oldestSafeRollbackVersion}`,
      );
    }
    previous = entry;
    releases.push({ ...entry });
  }
  return { schemaVersion: ledger.schemaVersion, keeperReleases: releases };
}

/** Reads and validates the ledger at `path`. */
export function readKeeperLedger(path) {
  return parseKeeperLedger(readJsonFile(path));
}

/**
 * The manifest block for a release: the floor a rollback may reach plus the retention window.
 *
 * The floor comes from the ledger, which lists releases that already shipped a keeper. A release
 * that is not in it yet inherits the oldest recorded floor, so every keeper release points at the
 * first build that understood keeper-owned sessions: no build older than that can discover or
 * service them. On an empty ledger the release publishes itself, because then it *is* the first
 * build with a keeper.
 *
 * Deriving the floor from the ledger's oldest entry (rather than from the release's own position)
 * is also what keeps a published `latest.json` reproducible: re-assembling release N after the
 * ledger recorded N yields the floor N published, so byte-comparison of the re-derived manifest
 * cannot drift.
 */
export function keeperManifest({ appVersion, ledger }) {
  if (!isKeeperVersion(appVersion)) {
    throw new Error(`keeper manifest needs a three-component appVersion, got ${JSON.stringify(appVersion)}`);
  }
  const parsed = parseKeeperLedger(ledger);
  const entry = parsed.keeperReleases.find((release) => release.appVersion === appVersion);
  const oldest = parsed.keeperReleases[0] ?? null;
  const newest = parsed.keeperReleases[parsed.keeperReleases.length - 1] ?? null;
  if (!entry && newest && compareKeeperVersions(appVersion, newest.appVersion) <= 0) {
    throw new Error(
      `release ${appVersion} is not newer than the latest recorded keeper release ${newest.appVersion}; ` +
        `a keeper release that already shipped cannot be re-cut`,
    );
  }
  const floor = entry ? entry.oldestSafeRollbackVersion : (oldest?.oldestSafeRollbackVersion ?? appVersion);
  return {
    [OLDEST_SAFE_ROLLBACK_KEY]: floor,
    protocolMajor: KEEPER_PROTOCOL_MAJOR,
    retainedGenerations: KEEPER_RETAINED_GENERATIONS,
  };
}

/**
 * The ledger with `appVersion` appended as the newest keeper release. Called after a release ships
 * so the next one keeps the same floor. Appending an already recorded release returns the ledger
 * unchanged.
 *
 * The recorded floor defaults to the one `keeperManifest` published for this release, so the ledger
 * can never disagree with the `latest.json` that already went out.
 */
export function recordKeeperRelease(ledger, { appVersion, oldestSafeRollbackVersion }) {
  const parsed = parseKeeperLedger(ledger);
  if (parsed.keeperReleases.some((release) => release.appVersion === appVersion)) {
    return parsed;
  }
  const floor =
    oldestSafeRollbackVersion ?? keeperManifest({ appVersion, ledger: parsed })[OLDEST_SAFE_ROLLBACK_KEY];
  return parseKeeperLedger({
    schemaVersion: KEEPER_POLICY_SCHEMA_VERSION,
    keeperReleases: [
      ...parsed.keeperReleases,
      { appVersion, protocolMajor: KEEPER_PROTOCOL_MAJOR, oldestSafeRollbackVersion: floor },
    ],
  });
}
